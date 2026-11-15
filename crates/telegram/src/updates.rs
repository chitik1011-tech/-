use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use grammers_client::{
    Client, InvocationError, client::UpdateStream, sender::UpdatesConfiguration,
    session::updates::UpdatesLike, tl, update::Update,
};
pub use tg_mobile_core::ConnectionState;
use tg_mobile_core::{
    BackendUpdateSender, ConnectionToken, CoreError, CoreEvent, LinkEvent, MessageSummary,
    PollSummary,
};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::sleep;

use crate::{
    chats::{PeerRegistry, remember_peer},
    ids::channel_id_to_core,
    messages::{MessageDeduplicator, message_to_summary},
    polls::{
        PollRegistry, forget_deleted_polls, poll_from_message, poll_from_update, remember_poll,
    },
};

/// Backend-native update model.
///
/// It remains richer than `CoreEvent`; supported message mutations, backend
/// errors, and connection state cross the core boundary, while chat/unread and
/// typing updates remain backend-native until their core models are added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramUpdate {
    MessageAdded(MessageSummary),
    MessageEdited(MessageSummary),
    MessagesDeleted {
        chat_id: Option<i64>,
        message_ids: Vec<i64>,
    },
    PollUpdated(PollSummary),
    ChatChanged {
        chat_id: i64,
    },
    UnreadCountChanged {
        chat_id: i64,
        unread_count: u32,
    },
    Typing {
        chat_id: i64,
        sender_id: i64,
        active: bool,
    },
    ConnectionStateChanged(ConnectionState),
    ConnectionLifecycle {
        token: ConnectionToken,
        event: LinkEvent,
    },
    Error(String),
    Unhandled,
}

impl TelegramUpdate {
    pub(crate) fn to_core_event(&self) -> Option<CoreEvent> {
        match self {
            Self::MessageAdded(message) => Some(CoreEvent::MessageAdded(message.clone())),
            Self::MessageEdited(message) => Some(CoreEvent::MessageEdited(message.clone())),
            Self::MessagesDeleted {
                chat_id,
                message_ids,
            } => Some(CoreEvent::MessagesDeleted {
                chat_id: *chat_id,
                message_ids: message_ids.clone(),
            }),
            Self::PollUpdated(poll) => Some(CoreEvent::PollUpdated(poll.clone())),
            Self::ConnectionStateChanged(_) => None,
            Self::ConnectionLifecycle { token, event } => Some(CoreEvent::ConnectionLifecycle {
                token: *token,
                event: *event,
            }),
            Self::Error(message) => Some(CoreEvent::Error(message.clone())),
            _ => None,
        }
    }
}

pub(crate) struct UpdateWorkerInput {
    pub client: Client,
    pub network_updates: mpsc::Receiver<UpdatesLike>,
    pub catch_up: bool,
    pub peers: PeerRegistry,
    pub dedup: Arc<MessageDeduplicator>,
    pub polls: PollRegistry,
    pub events: broadcast::Sender<TelegramUpdate>,
    pub core_updates: Option<BackendUpdateSender>,
    pub connection_token: watch::Receiver<Option<ConnectionToken>>,
    pub shutdown: watch::Receiver<bool>,
}

const UPDATE_RETRY_INITIAL_DELAY: Duration = Duration::from_millis(250);
const UPDATE_RETRY_MAX_DELAY: Duration = Duration::from_secs(30);

trait UpdateSource {
    type Item;

    fn next(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Item, InvocationError>> + Send + '_>>;
}

impl UpdateSource for UpdateStream {
    type Item = Update;

    fn next(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Item, InvocationError>> + Send + '_>> {
        Box::pin(UpdateStream::next(self))
    }
}

#[derive(Debug)]
enum NextOutcome<T> {
    Item(T),
    Shutdown,
    Terminal(InvocationError),
}

#[derive(Debug, Clone, Copy)]
struct RetryBackoff {
    next_delay: Duration,
}

impl Default for RetryBackoff {
    fn default() -> Self {
        Self {
            next_delay: UPDATE_RETRY_INITIAL_DELAY,
        }
    }
}

impl RetryBackoff {
    fn on_transient_error(&mut self) -> Duration {
        let delay = self.next_delay;
        self.next_delay = delay
            .checked_mul(2)
            .unwrap_or(UPDATE_RETRY_MAX_DELAY)
            .min(UPDATE_RETRY_MAX_DELAY);
        delay
    }

    fn on_success(&mut self) {
        self.next_delay = UPDATE_RETRY_INITIAL_DELAY;
    }
}

fn is_transient_update_error(error: &InvocationError) -> bool {
    match error {
        InvocationError::Io(_) => true,
        InvocationError::Rpc(error) => {
            error.code >= 500
                || error.code == 420
                || error.name.starts_with("FLOOD_WAIT_")
                || matches!(error.name.as_str(), "RPC_CALL_FAIL" | "RPC_MCGET_FAIL")
        }
        InvocationError::Transport(grammers_mtproto::transport::Error::BadStatus {
            status: 429,
        }) => true,
        // A dropped invocation is the signal grammers uses when the sender
        // pool or the updates receiver has closed. It cannot recover by polling
        // this same stream again.
        InvocationError::Dropped(_) => false,
        InvocationError::Session(_)
        | InvocationError::Deserialize(_)
        | InvocationError::Transport(_)
        | InvocationError::InvalidDc
        | InvocationError::Authentication(_) => false,
    }
}

async fn next_with_recovery<S, F, Fut>(
    stream: &mut S,
    shutdown: &mut watch::Receiver<bool>,
    backoff: &mut RetryBackoff,
    mut report_transient: F,
) -> NextOutcome<S::Item>
where
    S: UpdateSource,
    F: FnMut(String) -> Fut,
    Fut: Future<Output = ()>,
{
    loop {
        if *shutdown.borrow() {
            return NextOutcome::Shutdown;
        }

        let result = tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return NextOutcome::Shutdown;
                }
                continue;
            }
            result = stream.next() => result,
        };

        match result {
            Ok(update) => {
                backoff.on_success();
                return NextOutcome::Item(update);
            }
            Err(error) if is_transient_update_error(&error) => {
                report_transient(format!("Telegram update stream (retrying): {error}")).await;
                let delay = backoff.on_transient_error();
                tokio::select! {
                    biased;
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            return NextOutcome::Shutdown;
                        }
                    }
                    _ = sleep(delay) => {}
                }
            }
            Err(error) => return NextOutcome::Terminal(error),
        }
    }
}

pub(crate) async fn run_update_worker(mut input: UpdateWorkerInput) {
    let handshake_token = *input.connection_token.borrow();
    let worker_generation = handshake_token.map(|token| token.generation());
    let mut active_token = handshake_token;
    emit_update(
        &input.events,
        &input.core_updates,
        TelegramUpdate::ConnectionStateChanged(ConnectionState::Connecting),
        None,
    )
    .await;

    let mut stream = match input
        .client
        .stream_updates(
            input.network_updates,
            UpdatesConfiguration {
                catch_up: input.catch_up,
                ..Default::default()
            },
        )
        .await
    {
        Ok(stream) => stream,
        Err(error) => {
            emit_error(
                &input.events,
                &input.core_updates,
                format!("create Telegram update stream: {error}"),
                handshake_token,
            )
            .await;
            emit_update(
                &input.events,
                &input.core_updates,
                TelegramUpdate::ConnectionStateChanged(ConnectionState::Disconnected),
                None,
            )
            .await;
            if let Some(token) = handshake_token {
                emit_lifecycle(
                    &input.events,
                    &input.core_updates,
                    token,
                    LinkEvent::ConnectFailed { retryable: false },
                )
                .await;
            }
            return;
        }
    };

    if worker_generation.is_some_and(|generation| {
        input
            .connection_token
            .borrow()
            .map(|token| token.generation())
            != Some(generation)
    }) {
        return;
    }

    if let Some(token) = active_token {
        active_token = emit_lifecycle(
            &input.events,
            &input.core_updates,
            token,
            LinkEvent::TransportConnected,
        )
        .await;
        if let Some(token) = active_token {
            active_token = emit_lifecycle(
                &input.events,
                &input.core_updates,
                token,
                LinkEvent::SyncStarted,
            )
            .await;
        }
    }

    let mut backoff = RetryBackoff::default();
    loop {
        let observed_token = match (active_token, worker_generation) {
            (Some(expected), Some(generation)) => {
                wait_for_current_attempt(&mut input.connection_token, expected, generation).await
            }
            _ => *input.connection_token.borrow(),
        };
        if worker_generation.is_some_and(|generation| {
            observed_token.map(|token| token.generation()) != Some(generation)
        }) {
            break;
        }
        if observed_token.is_some() {
            active_token = observed_token;
        }
        let operation_token = active_token;
        let transient_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let transient_flag = Arc::clone(&transient_seen);
        let events = &input.events;
        let core_updates = &input.core_updates;
        let connection_token = &input.connection_token;
        let outcome =
            next_with_recovery(&mut stream, &mut input.shutdown, &mut backoff, |message| {
                let transient_flag = Arc::clone(&transient_flag);
                async move {
                    emit_error(events, core_updates, message, operation_token).await;
                    if let Some(token) = operation_token {
                        if *connection_token.borrow() == Some(token) {
                            emit_lifecycle(events, core_updates, token, LinkEvent::TransportLost)
                                .await;
                            transient_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                }
            })
            .await;

        let observed_token = *input.connection_token.borrow();
        if worker_generation.is_some_and(|generation| {
            observed_token.map(|token| token.generation()) != Some(generation)
        }) {
            break;
        }
        if transient_seen.load(std::sync::atomic::Ordering::SeqCst) {
            active_token = operation_token.map(ConnectionToken::next_epoch);
        } else if observed_token.is_some() {
            active_token = observed_token;
        }

        match outcome {
            NextOutcome::Item(update) => {
                if active_token != operation_token {
                    if let Some(token) = active_token {
                        active_token = emit_lifecycle(
                            &input.events,
                            &input.core_updates,
                            token,
                            LinkEvent::Retry,
                        )
                        .await;
                    }
                    if let Some(token) = active_token {
                        active_token = emit_lifecycle(
                            &input.events,
                            &input.core_updates,
                            token,
                            LinkEvent::TransportConnected,
                        )
                        .await;
                    }
                    if let Some(token) = active_token {
                        active_token = emit_lifecycle(
                            &input.events,
                            &input.core_updates,
                            token,
                            LinkEvent::SyncStarted,
                        )
                        .await;
                    }
                }
                match map_update(update, &input.peers, &input.dedup, &input.polls).await {
                    Ok(events) => {
                        for event in events {
                            emit_update(&input.events, &input.core_updates, event, operation_token)
                                .await;
                        }
                    }
                    Err(error) => {
                        emit_error(
                            &input.events,
                            &input.core_updates,
                            format!("map Telegram update: {error}"),
                            operation_token,
                        )
                        .await;
                    }
                }
            }
            NextOutcome::Shutdown => break,
            NextOutcome::Terminal(error) => {
                emit_error(
                    &input.events,
                    &input.core_updates,
                    format!("Telegram update stream: {error}"),
                    operation_token,
                )
                .await;
                if active_token == operation_token {
                    if let Some(token) = active_token {
                        active_token = emit_lifecycle(
                            &input.events,
                            &input.core_updates,
                            token,
                            LinkEvent::TransportLost,
                        )
                        .await;
                    }
                }
                if let Some(token) = active_token {
                    emit_lifecycle(
                        &input.events,
                        &input.core_updates,
                        token,
                        LinkEvent::ConnectFailed { retryable: false },
                    )
                    .await;
                }
                break;
            }
        }
    }

    if let Err(error) = stream.sync_update_state().await {
        emit_error(
            &input.events,
            &input.core_updates,
            format!("persist Telegram update state: {error}"),
            active_token,
        )
        .await;
    }
    emit_update(
        &input.events,
        &input.core_updates,
        TelegramUpdate::ConnectionStateChanged(ConnectionState::Disconnected),
        None,
    )
    .await;
}

async fn emit_update(
    native: &broadcast::Sender<TelegramUpdate>,
    core: &Option<BackendUpdateSender>,
    update: TelegramUpdate,
    token: Option<ConnectionToken>,
) {
    let core_event = update.to_core_event();
    let _ = native.send(update);
    if let (Some(core), Some(event), Some(token)) = (core.as_ref(), core_event, token) {
        let _ = core
            .send(Ok(CoreEvent::SessionUpdate {
                token,
                event: Box::new(event),
            }))
            .await;
    }
}

async fn emit_lifecycle(
    native: &broadcast::Sender<TelegramUpdate>,
    core: &Option<BackendUpdateSender>,
    token: ConnectionToken,
    event: LinkEvent,
) -> Option<ConnectionToken> {
    let state = match event {
        LinkEvent::TransportConnected | LinkEvent::SyncStarted | LinkEvent::Retry => {
            Some(ConnectionState::Connecting)
        }
        LinkEvent::SyncCompleted => Some(ConnectionState::Ready),
        LinkEvent::TransportLost | LinkEvent::ConnectFailed { .. } => {
            Some(ConnectionState::Disconnected)
        }
        LinkEvent::Logout | LinkEvent::Shutdown => Some(ConnectionState::Disconnected),
    };
    if let Some(state) = state {
        let _ = native.send(TelegramUpdate::ConnectionStateChanged(state));
    }

    let update = TelegramUpdate::ConnectionLifecycle { token, event };
    let _ = native.send(update.clone());
    if let (Some(core), Some(core_event)) = (core.as_ref(), update.to_core_event()) {
        let _ = core
            .send(Ok(CoreEvent::SessionUpdate {
                token,
                event: Box::new(core_event),
            }))
            .await;
    }
    Some(
        if matches!(
            event,
            LinkEvent::TransportLost
                | LinkEvent::ConnectFailed { .. }
                | LinkEvent::Logout
                | LinkEvent::Shutdown
        ) {
            token.next_epoch()
        } else {
            token
        },
    )
}

async fn wait_for_current_attempt(
    token_rx: &mut watch::Receiver<Option<ConnectionToken>>,
    expected: ConnectionToken,
    generation: tg_mobile_core::SessionGeneration,
) -> Option<ConnectionToken> {
    loop {
        let current = *token_rx.borrow();
        match current {
            Some(observed) if observed.generation() != generation => return None,
            Some(observed) if observed.epoch() >= expected.epoch() => return Some(observed),
            _ => {
                if token_rx.changed().await.is_err() {
                    return None;
                }
            }
        }
    }
}

async fn emit_error(
    native: &broadcast::Sender<TelegramUpdate>,
    core: &Option<BackendUpdateSender>,
    message: String,
    token: Option<ConnectionToken>,
) {
    let _ = native.send(TelegramUpdate::Error(message.clone()));
    if let Some(core) = core.as_ref() {
        let _ = match token {
            Some(token) => {
                core.send(Ok(CoreEvent::SessionUpdate {
                    token,
                    event: Box::new(CoreEvent::Error(message)),
                }))
                .await
            }
            None => core.send(Err(CoreError::Backend(message))).await,
        };
    }
}

async fn map_update(
    update: Update,
    peers: &PeerRegistry,
    dedup: &MessageDeduplicator,
    polls: &PollRegistry,
) -> tg_mobile_core::Result<Vec<TelegramUpdate>> {
    match update {
        Update::NewMessage(message) => {
            let summary = message_to_summary(&message);
            if let Ok(Some(peer)) = message.peer_ref().await {
                remember_peer(peers, summary.chat_id, peer).await;
            }

            let poll = poll_from_message(&message)?;
            if let Some(poll) = poll.as_ref() {
                remember_poll(polls, poll.clone()).await;
            }

            let mut events = Vec::with_capacity(2);
            if dedup.should_emit(&summary).await {
                events.push(TelegramUpdate::MessageAdded(summary));
            }
            if let Some(poll) = poll {
                events.push(TelegramUpdate::PollUpdated(poll));
            }
            Ok(events)
        }
        Update::MessageEdited(message) => {
            let summary = message_to_summary(&message);
            if let Ok(Some(peer)) = message.peer_ref().await {
                remember_peer(peers, summary.chat_id, peer).await;
            }

            let poll = poll_from_message(&message)?;
            if let Some(poll) = poll.as_ref() {
                remember_poll(polls, poll.clone()).await;
            }

            let mut events = vec![TelegramUpdate::MessageEdited(summary)];
            if let Some(poll) = poll {
                events.push(TelegramUpdate::PollUpdated(poll));
            }
            Ok(events)
        }
        Update::MessageDeleted(deletion) => {
            let chat_id = deletion.channel_id().map(channel_id_to_core);
            let message_ids = deletion
                .messages()
                .iter()
                .copied()
                .map(i64::from)
                .collect::<Vec<_>>();
            forget_deleted_polls(polls, chat_id, &message_ids).await;
            Ok(vec![TelegramUpdate::MessagesDeleted {
                chat_id,
                message_ids,
            }])
        }
        Update::Raw(raw) => {
            if let tl::enums::Update::MessagePoll(update) = &raw.raw {
                Ok(poll_from_update(update, polls)
                    .await?
                    .map(TelegramUpdate::PollUpdated)
                    .into_iter()
                    .collect())
            } else {
                Ok(vec![TelegramUpdate::Unhandled])
            }
        }
        _ => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, future::Future, pin::Pin, sync::Arc};

    use grammers_client::{InvocationError, sender::RpcError};
    use tokio::sync::watch;

    use super::{ConnectionState, TelegramUpdate};
    use super::{
        NextOutcome, RetryBackoff, UPDATE_RETRY_INITIAL_DELAY, UPDATE_RETRY_MAX_DELAY,
        UpdateSource, is_transient_update_error, next_with_recovery, wait_for_current_attempt,
    };
    use tg_mobile_core::{
        ConnectionMachine, CoreEvent, LinkEvent, MessageSummary, backend_update_channel,
    };

    struct FakeUpdateSource {
        results: VecDeque<Result<u8, InvocationError>>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl UpdateSource for FakeUpdateSource {
        type Item = u8;

        fn next(
            &mut self,
        ) -> Pin<Box<dyn Future<Output = Result<Self::Item, InvocationError>> + Send + '_>>
        {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let result = self
                .results
                .pop_front()
                .unwrap_or(Err(InvocationError::Dropped("fake source exhausted")));
            Box::pin(async move { result })
        }
    }

    #[test]
    fn transient_update_errors_retry_same_source_and_success_resets_backoff() {
        let mut backoff = RetryBackoff::default();
        assert_eq!(backoff.on_transient_error(), UPDATE_RETRY_INITIAL_DELAY);
        assert_eq!(backoff.on_transient_error(), UPDATE_RETRY_INITIAL_DELAY * 2);
        backoff.on_success();
        assert_eq!(backoff.on_transient_error(), UPDATE_RETRY_INITIAL_DELAY);
        for _ in 0..16 {
            backoff.on_transient_error();
        }
        assert!(backoff.next_delay <= UPDATE_RETRY_MAX_DELAY);
    }

    #[tokio::test(start_paused = true)]
    async fn transient_error_retries_the_same_stream_until_an_update_arrives() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut source = FakeUpdateSource {
            results: VecDeque::from([
                Err(InvocationError::Io(std::io::Error::other("temporary"))),
                Ok(7),
            ]),
            calls: Arc::clone(&calls),
        };
        let (_shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let mut backoff = RetryBackoff::default();
        let result =
            next_with_recovery(&mut source, &mut shutdown_rx, &mut backoff, |_| async {}).await;

        assert!(matches!(result, NextOutcome::Item(7)));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(backoff.next_delay, UPDATE_RETRY_INITIAL_DELAY);
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_interrupts_retry_backoff_sleep() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut source = FakeUpdateSource {
            results: VecDeque::from([Err(InvocationError::Io(std::io::Error::other("temporary")))]),
            calls: Arc::clone(&calls),
        };
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let mut backoff = RetryBackoff::default();
        let task = tokio::spawn(async move {
            next_with_recovery(&mut source, &mut shutdown_rx, &mut backoff, |_| async {}).await
        });
        tokio::task::yield_now().await;
        shutdown_tx.send(true).unwrap();

        assert!(matches!(task.await.unwrap(), NextOutcome::Shutdown));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dropped_update_stream_is_terminal_without_retrying() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut source = FakeUpdateSource {
            results: VecDeque::from([Err(InvocationError::Dropped("updates channel was closed"))]),
            calls: Arc::clone(&calls),
        };
        let (_shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let mut backoff = RetryBackoff::default();
        let result =
            next_with_recovery(&mut source, &mut shutdown_rx, &mut backoff, |_| async {}).await;

        assert!(matches!(result, NextOutcome::Terminal(_)));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn only_transient_io_and_server_errors_are_retried() {
        assert!(is_transient_update_error(&InvocationError::Io(
            std::io::Error::other("temporary")
        )));
        assert!(is_transient_update_error(&InvocationError::Rpc(RpcError {
            code: 420,
            name: "FLOOD_WAIT_5".into(),
            value: Some(5),
            caused_by: None,
        })));
        assert!(is_transient_update_error(&InvocationError::Rpc(RpcError {
            code: 500,
            name: "INTERNAL".into(),
            value: None,
            caused_by: None,
        })));
        assert!(is_transient_update_error(&InvocationError::Transport(
            grammers_mtproto::transport::Error::BadStatus { status: 429 }
        )));
        assert!(!is_transient_update_error(&InvocationError::Rpc(
            RpcError {
                code: 400,
                name: "CHAT_WRITE_FORBIDDEN".into(),
                value: None,
                caused_by: None,
            }
        )));
        assert!(!is_transient_update_error(&InvocationError::Dropped(
            "updates channel was closed"
        )));
        assert!(!is_transient_update_error(&InvocationError::Transport(
            grammers_mtproto::transport::Error::BadStatus { status: 404 }
        )));
        assert!(!is_transient_update_error(&InvocationError::Transport(
            grammers_mtproto::transport::Error::BadCrc {
                expected: 1,
                got: 2,
            }
        )));
    }

    #[test]
    fn only_supported_update_types_cross_current_core_boundary() {
        let message = MessageSummary {
            id: 1,
            chat_id: 2,
            sender_id: 3,
            text: "hello".into(),
            timestamp_unix: 4,
            outgoing: false,
            attachments: Vec::new(),
            forwarded_from: None,
            edited: false,
            edit_timestamp_unix: None,
            service_message: false,
            service: None,
            media_caption: None,
            reply_to: None,
        };
        assert!(matches!(
            TelegramUpdate::MessageAdded(message.clone()).to_core_event(),
            Some(CoreEvent::MessageAdded(_))
        ));
        assert_eq!(
            TelegramUpdate::MessageEdited(message.clone()).to_core_event(),
            Some(CoreEvent::MessageEdited(message))
        );
        assert_eq!(
            TelegramUpdate::MessagesDeleted {
                chat_id: Some(2),
                message_ids: vec![1, 2],
            }
            .to_core_event(),
            Some(CoreEvent::MessagesDeleted {
                chat_id: Some(2),
                message_ids: vec![1, 2],
            })
        );
        assert_eq!(
            TelegramUpdate::ConnectionStateChanged(ConnectionState::Ready).to_core_event(),
            None
        );
        let mut machine = ConnectionMachine::default();
        machine.start_session();
        let token = machine.token();
        assert_eq!(
            TelegramUpdate::ConnectionLifecycle {
                token,
                event: LinkEvent::SyncCompleted,
            }
            .to_core_event(),
            Some(CoreEvent::ConnectionLifecycle {
                token,
                event: LinkEvent::SyncCompleted,
            })
        );
        assert!(TelegramUpdate::Unhandled.to_core_event().is_none());
    }

    #[tokio::test]
    async fn session_update_keeps_the_token_captured_for_its_operation() {
        let mut machine = ConnectionMachine::default();
        machine.start_session();
        let operation_token = machine.token();
        let message = MessageSummary {
            id: 1,
            chat_id: 2,
            sender_id: 3,
            text: "message".into(),
            timestamp_unix: 4,
            outgoing: false,
            attachments: Vec::new(),
            forwarded_from: None,
            edited: false,
            edit_timestamp_unix: None,
            service_message: false,
            service: None,
            media_caption: None,
            reply_to: None,
        };
        let (core_tx, mut core_rx) = backend_update_channel();
        let (native_tx, _) = tokio::sync::broadcast::channel(1);

        super::emit_update(
            &native_tx,
            &Some(core_tx),
            TelegramUpdate::MessageAdded(message.clone()),
            Some(operation_token),
        )
        .await;

        assert_eq!(
            core_rx.recv().await.unwrap().unwrap(),
            CoreEvent::SessionUpdate {
                token: operation_token,
                event: Box::new(CoreEvent::MessageAdded(message)),
            }
        );
    }

    #[tokio::test]
    async fn retry_uses_transport_lost_epoch_for_following_lifecycle_events() {
        let mut machine = ConnectionMachine::default();
        machine.start_session();
        let initial = machine.token();
        for event in [
            LinkEvent::TransportConnected,
            LinkEvent::SyncStarted,
            LinkEvent::SyncCompleted,
        ] {
            machine.apply(initial, event).unwrap();
        }

        let (core_tx, mut core_rx) = backend_update_channel();
        let (native_tx, _) = tokio::sync::broadcast::channel(8);
        let after_loss = super::emit_lifecycle(
            &native_tx,
            &Some(core_tx.clone()),
            initial,
            LinkEvent::TransportLost,
        )
        .await
        .unwrap();
        let lost = core_rx.recv().await.unwrap().unwrap();
        assert!(matches!(
            lost,
            CoreEvent::SessionUpdate {
                token,
                event
            } if token == initial && matches!(*event, CoreEvent::ConnectionLifecycle {
                token: event_token,
                event: LinkEvent::TransportLost
            } if event_token == initial)
        ));
        machine.apply(initial, LinkEvent::TransportLost).unwrap();
        assert_eq!(after_loss, machine.token());

        let after_retry = super::emit_lifecycle(
            &native_tx,
            &Some(core_tx.clone()),
            after_loss,
            LinkEvent::Retry,
        )
        .await
        .unwrap();
        let retry = core_rx.recv().await.unwrap().unwrap();
        assert!(matches!(
            retry,
            CoreEvent::SessionUpdate {
                token,
                event
            } if token == after_loss && matches!(*event, CoreEvent::ConnectionLifecycle {
                token: event_token,
                event: LinkEvent::Retry
            } if event_token == after_loss)
        ));
        machine.apply(after_loss, LinkEvent::Retry).unwrap();
        assert_eq!(after_retry, machine.token());

        for event in [LinkEvent::TransportConnected, LinkEvent::SyncStarted] {
            let returned =
                super::emit_lifecycle(&native_tx, &Some(core_tx.clone()), after_retry, event)
                    .await
                    .unwrap();
            let update = core_rx.recv().await.unwrap().unwrap();
            assert!(matches!(
                update,
                CoreEvent::SessionUpdate { token, event: boxed }
                    if token == after_retry && matches!(
                        *boxed,
                        CoreEvent::ConnectionLifecycle { token: event_token, event: lifecycle }
                            if event_token == after_retry && lifecycle == event
                    )
            ));
            machine.apply(after_retry, event).unwrap();
            assert_eq!(returned, after_retry);
            assert_eq!(machine.token(), after_retry);
        }
        assert_eq!(machine.phase(), tg_mobile_core::LinkPhase::Synchronizing);
    }

    #[tokio::test]
    async fn next_attempt_waits_for_the_actor_owned_token() {
        let mut machine = ConnectionMachine::default();
        machine.start_session();
        let old = machine.token();
        let expected = old.next_epoch();
        let (sender, mut receiver) = watch::channel(Some(old));
        let generation = old.generation();

        let task = tokio::spawn(async move {
            wait_for_current_attempt(&mut receiver, expected, generation).await
        });
        tokio::task::yield_now().await;
        sender.send_replace(Some(expected));

        assert_eq!(task.await.unwrap(), Some(expected));
    }
}
