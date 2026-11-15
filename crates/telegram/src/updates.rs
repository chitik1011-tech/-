use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use grammers_client::{
    Client, InvocationError, client::UpdateStream, sender::UpdatesConfiguration,
    session::updates::UpdatesLike, update::Update,
};
pub use tg_mobile_core::ConnectionState;
use tg_mobile_core::{BackendUpdateSender, CoreError, CoreEvent, MessageSummary};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::sleep;

use crate::{
    chats::{PeerRegistry, remember_peer},
    ids::channel_id_to_core,
    messages::{MessageDeduplicator, message_to_summary},
};

/// Backend-native update model.
///
/// It is intentionally richer than `CoreEvent`; message edits/deletes and
/// several chat-level updates remain backend-native, while connection state
/// and supported message/error updates cross the core boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramUpdate {
    MessageAdded(MessageSummary),
    MessageEdited(MessageSummary),
    MessagesDeleted {
        chat_id: Option<i64>,
        message_ids: Vec<i64>,
    },
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
    Error(String),
    Unhandled,
}

impl TelegramUpdate {
    pub(crate) fn to_core_event(&self) -> Option<CoreEvent> {
        match self {
            Self::MessageAdded(message) => Some(CoreEvent::MessageAdded(message.clone())),
            Self::ConnectionStateChanged(state) => Some(CoreEvent::ConnectionStateChanged(*state)),
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
    pub events: broadcast::Sender<TelegramUpdate>,
    pub core_updates: Option<BackendUpdateSender>,
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
    emit_update(
        &input.events,
        &input.core_updates,
        TelegramUpdate::ConnectionStateChanged(ConnectionState::Connecting),
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
            )
            .await;
            emit_update(
                &input.events,
                &input.core_updates,
                TelegramUpdate::ConnectionStateChanged(ConnectionState::Disconnected),
            )
            .await;
            return;
        }
    };

    emit_update(
        &input.events,
        &input.core_updates,
        TelegramUpdate::ConnectionStateChanged(ConnectionState::Ready),
    )
    .await;

    let mut backoff = RetryBackoff::default();
    loop {
        let events = &input.events;
        let core_updates = &input.core_updates;
        let outcome = next_with_recovery(
            &mut stream,
            &mut input.shutdown,
            &mut backoff,
            |message| async move { emit_error(events, core_updates, message).await },
        )
        .await;
        match outcome {
            NextOutcome::Item(update) => {
                if let Some(event) = map_update(update, &input.peers, &input.dedup).await {
                    emit_update(&input.events, &input.core_updates, event).await;
                }
            }
            NextOutcome::Shutdown => break,
            NextOutcome::Terminal(error) => {
                emit_error(
                    &input.events,
                    &input.core_updates,
                    format!("Telegram update stream: {error}"),
                )
                .await;
                break;
            }
        }
    }

    if let Err(error) = stream.sync_update_state().await {
        emit_error(
            &input.events,
            &input.core_updates,
            format!("persist Telegram update state: {error}"),
        )
        .await;
    }
    emit_update(
        &input.events,
        &input.core_updates,
        TelegramUpdate::ConnectionStateChanged(ConnectionState::Disconnected),
    )
    .await;
}

async fn emit_update(
    native: &broadcast::Sender<TelegramUpdate>,
    core: &Option<BackendUpdateSender>,
    update: TelegramUpdate,
) {
    let core_event = update.to_core_event();
    let _ = native.send(update);
    if let (Some(core), Some(event)) = (core.as_ref(), core_event) {
        let _ = core.send(Ok(event)).await;
    }
}

async fn emit_error(
    native: &broadcast::Sender<TelegramUpdate>,
    core: &Option<BackendUpdateSender>,
    message: String,
) {
    let _ = native.send(TelegramUpdate::Error(message.clone()));
    if let Some(core) = core.as_ref() {
        let _ = core.send(Err(CoreError::Backend(message))).await;
    }
}

async fn map_update(
    update: Update,
    peers: &PeerRegistry,
    dedup: &MessageDeduplicator,
) -> Option<TelegramUpdate> {
    match update {
        Update::NewMessage(message) => {
            let summary = message_to_summary(&message);
            if let Ok(Some(peer)) = message.peer_ref().await {
                remember_peer(peers, summary.chat_id, peer).await;
            }
            dedup
                .should_emit(&summary)
                .await
                .then_some(TelegramUpdate::MessageAdded(summary))
        }
        Update::MessageEdited(message) => {
            let summary = message_to_summary(&message);
            if let Ok(Some(peer)) = message.peer_ref().await {
                remember_peer(peers, summary.chat_id, peer).await;
            }
            Some(TelegramUpdate::MessageEdited(summary))
        }
        Update::MessageDeleted(deletion) => Some(TelegramUpdate::MessagesDeleted {
            chat_id: deletion.channel_id().map(channel_id_to_core),
            message_ids: deletion.messages().iter().copied().map(i64::from).collect(),
        }),
        Update::Raw(_) => Some(TelegramUpdate::Unhandled),
        _ => None,
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
        UpdateSource, is_transient_update_error, next_with_recovery,
    };
    use tg_mobile_core::{CoreEvent, MessageSummary};

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
            media_caption: None,
            reply_to: None,
        };
        assert!(matches!(
            TelegramUpdate::MessageAdded(message).to_core_event(),
            Some(CoreEvent::MessageAdded(_))
        ));
        assert_eq!(
            TelegramUpdate::ConnectionStateChanged(ConnectionState::Ready).to_core_event(),
            Some(CoreEvent::ConnectionStateChanged(ConnectionState::Ready))
        );
        assert!(TelegramUpdate::Unhandled.to_core_event().is_none());
    }
}
