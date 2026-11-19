use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use grammers_client::{
    Client, InvocationError, client::UpdateStream, sender::UpdatesConfiguration,
    session::updates::UpdatesLike, tl, update::Update,
};
pub use tg_mobile_core::ConnectionState;
use tg_mobile_core::{
    BackendUpdateSender, ChatActivityKind, ConnectionToken, CoreError, CoreEvent,
    DEFAULT_CHAT_ACTIVITY_TTL_MS, LinkEvent, MessageReaction, MessageSummary, PollSummary,
    ReactionRevision, ReadDirection,
};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::sleep;

use crate::{
    chats::{PeerRegistry, remember_peer},
    ids::{PeerClass, channel_id_to_core, encode_peer},
    messages::{MessageDeduplicator, map_reactions, message_to_summary},
    polls::{
        PollRegistry, forget_deleted_polls, poll_from_message, poll_from_update, remember_poll,
    },
};

/// Backend-native update model.
///
/// It remains richer than `CoreEvent`; supported message mutations, activity,
/// backend errors, and connection state cross the core boundary, while other
/// chat and unread updates remain backend-native.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramUpdate {
    MessageAdded(MessageSummary),
    MessageEdited(MessageSummary),
    MessageReactionsUpdated {
        chat_id: i64,
        message_id: i64,
        reactions: Vec<MessageReaction>,
        revision: ReactionRevision,
    },
    MessagesDeleted {
        chat_id: Option<i64>,
        message_ids: Vec<i64>,
    },
    PollUpdated(PollSummary),
    PinnedMessagesChanged {
        chat_id: i64,
        message_ids: Vec<i64>,
        pinned: bool,
        pts: i32,
    },
    ContactsRefreshRequested,
    ChatUnreadMarkChanged {
        chat_id: i64,
        unread: bool,
    },
    ChatChanged {
        chat_id: i64,
    },
    UnreadCountChanged {
        chat_id: i64,
        unread_count: u32,
    },
    ReadReceiptUpdated {
        chat_id: i64,
        direction: ReadDirection,
        max_message_id: i64,
        unread_count: Option<u32>,
    },
    Typing {
        chat_id: i64,
        sender_id: i64,
        generation: u64,
        activity: ChatActivityKind,
        ttl_ms: u64,
    },
    TypingEnded {
        chat_id: i64,
        sender_id: i64,
        generation: u64,
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
            Self::PinnedMessagesChanged {
                chat_id,
                message_ids,
                pinned,
                pts,
            } => Some(CoreEvent::PinnedMessagesChanged {
                chat_id: *chat_id,
                message_ids: message_ids.clone(),
                pinned: *pinned,
                pts: Some(*pts),
            }),
            Self::MessageReactionsUpdated {
                chat_id,
                message_id,
                reactions,
                revision,
            } => Some(CoreEvent::MessageReactionsUpdated {
                chat_id: *chat_id,
                message_id: *message_id,
                reactions: reactions.clone(),
                revision: *revision,
            }),
            Self::ContactsRefreshRequested => Some(CoreEvent::ContactsRefreshRequested),
            Self::ChatUnreadMarkChanged { chat_id, unread } => {
                Some(CoreEvent::ChatUnreadMarkChanged {
                    chat_id: *chat_id,
                    unread: *unread,
                })
            }
            Self::ReadReceiptUpdated {
                chat_id,
                direction,
                max_message_id,
                unread_count,
            } => Some(CoreEvent::ReadReceiptUpdated {
                chat_id: *chat_id,
                direction: *direction,
                max_message_id: *max_message_id,
                unread_count: *unread_count,
            }),
            Self::Typing {
                chat_id,
                sender_id,
                generation,
                activity,
                ttl_ms,
            } => Some(CoreEvent::ChatActivityChanged {
                chat_id: *chat_id,
                user_id: *sender_id,
                generation: *generation,
                activity: *activity,
                ttl_ms: *ttl_ms,
            }),
            Self::TypingEnded {
                chat_id,
                sender_id,
                generation,
            } => Some(CoreEvent::ChatActivityEnded {
                chat_id: *chat_id,
                user_id: *sender_id,
                generation: *generation,
            }),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivitySignal {
    Started(ChatActivityKind),
    Ended,
    Ignored,
}

/// Allocates transient activity generations for one update stream. The map is
/// session-worker local and never persisted; CoreEvent delivery is additionally
/// wrapped in the connection token captured by the update worker.
#[derive(Debug, Default)]
struct ActivityGenerations {
    next_generation: u64,
    active: BTreeMap<(i64, i64), ActiveActivityGeneration>,
}

#[derive(Debug, Clone, Copy)]
struct ActiveActivityGeneration {
    generation: u64,
    expires_at: Instant,
}

const MAX_TRACKED_CHAT_ACTIVITIES: usize = 4_096;

impl ActivityGenerations {
    fn prune_expired(&mut self, now: Instant) {
        self.active.retain(|_, activity| activity.expires_at > now);
    }
}

impl ActivityGenerations {
    fn update(
        &mut self,
        chat_id: i64,
        sender_id: i64,
        signal: ActivitySignal,
    ) -> Option<TelegramUpdate> {
        if chat_id == 0 || sender_id <= 0 {
            return None;
        }
        let now = Instant::now();
        self.prune_expired(now);
        let key = (chat_id, sender_id);
        match signal {
            ActivitySignal::Started(activity) => {
                self.next_generation = self.next_generation.checked_add(1)?;
                let generation = self.next_generation;
                if self.active.len() >= MAX_TRACKED_CHAT_ACTIVITIES {
                    if let Some(oldest) = self
                        .active
                        .iter()
                        .min_by_key(|(_, activity)| activity.expires_at)
                        .map(|(key, _)| *key)
                    {
                        self.active.remove(&oldest);
                    }
                }
                self.active.insert(
                    key,
                    ActiveActivityGeneration {
                        generation,
                        expires_at: now + Duration::from_millis(DEFAULT_CHAT_ACTIVITY_TTL_MS),
                    },
                );
                Some(TelegramUpdate::Typing {
                    chat_id,
                    sender_id,
                    generation,
                    activity,
                    ttl_ms: DEFAULT_CHAT_ACTIVITY_TTL_MS,
                })
            }
            ActivitySignal::Ended => {
                let generation = self.active.remove(&key)?.generation;
                Some(TelegramUpdate::TypingEnded {
                    chat_id,
                    sender_id,
                    generation,
                })
            }
            ActivitySignal::Ignored => None,
        }
    }
}

fn activity_signal(action: &tl::enums::SendMessageAction) -> ActivitySignal {
    use tl::enums::SendMessageAction as Action;
    match action {
        Action::SendMessageTypingAction => ActivitySignal::Started(ChatActivityKind::Typing),
        Action::SendMessageRecordAudioAction => {
            ActivitySignal::Started(ChatActivityKind::RecordingVoice)
        }
        Action::SendMessageUploadAudioAction(_)
        | Action::SendMessageUploadPhotoAction(_)
        | Action::SendMessageUploadDocumentAction(_)
        | Action::SendMessageUploadVideoAction(_)
        | Action::SendMessageUploadRoundAction(_) => {
            ActivitySignal::Started(ChatActivityKind::UploadingMedia)
        }
        Action::SendMessageCancelAction => ActivitySignal::Ended,
        Action::SendMessageRecordVideoAction | Action::SendMessageRecordRoundAction => {
            ActivitySignal::Started(ChatActivityKind::RecordingVideo)
        }
        _ => ActivitySignal::Ignored,
    }
}

fn map_raw_activity_update(
    update: &tl::enums::Update,
    generations: &mut ActivityGenerations,
) -> Option<TelegramUpdate> {
    use tl::enums::Update as RawUpdate;
    match update {
        RawUpdate::UserTyping(update) => generations.update(
            update.user_id,
            update.user_id,
            activity_signal(&update.action),
        ),
        RawUpdate::ChatUserTyping(update) => {
            if update.chat_id <= 0 {
                return None;
            }
            let sender_id = match &update.from_id {
                tl::enums::Peer::User(peer) => peer.user_id,
                _ => return None,
            };
            generations.update(-update.chat_id, sender_id, activity_signal(&update.action))
        }
        RawUpdate::ChannelUserTyping(update) => {
            if update.channel_id <= 0 {
                return None;
            }
            let sender_id = match &update.from_id {
                tl::enums::Peer::User(peer) => peer.user_id,
                _ => return None,
            };
            generations.update(
                crate::ids::channel_id_to_core(update.channel_id),
                sender_id,
                activity_signal(&update.action),
            )
        }
        _ => None,
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

fn raw_peer_to_core(peer: &tl::enums::Peer) -> i64 {
    match peer {
        tl::enums::Peer::User(peer) => encode_peer(PeerClass::User, peer.user_id),
        tl::enums::Peer::Chat(peer) => encode_peer(PeerClass::Chat, peer.chat_id),
        tl::enums::Peer::Channel(peer) => encode_peer(PeerClass::Channel, peer.channel_id),
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
    let mut activity_generations = ActivityGenerations::default();
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
    let mut update_sequence = 0_u64;
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
                update_sequence = update_sequence.saturating_add(1);
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
                let recovered_transport = transient_seen.load(std::sync::atomic::Ordering::SeqCst);
                if !update_token_is_current(operation_token, observed_token, recovered_transport) {
                    continue;
                }
                // The first update returned after a transient stream failure belongs to
                // the newly opened epoch, not the stale epoch that was awaiting next().
                let event_token = if recovered_transport {
                    operation_token.map(ConnectionToken::next_epoch)
                } else {
                    operation_token
                };
                let revision = ReactionRevision {
                    token: event_token.unwrap_or_default(),
                    sequence: update_sequence,
                };
                match map_update(
                    update,
                    &input.peers,
                    &input.dedup,
                    &input.polls,
                    revision,
                    &mut activity_generations,
                )
                .await
                {
                    Ok(events) => {
                        for event in events {
                            if update_token_is_current(
                                event_token,
                                *input.connection_token.borrow(),
                                recovered_transport,
                            ) {
                                emit_update(&input.events, &input.core_updates, event, event_token)
                                    .await;
                            }
                        }
                    }
                    Err(error) => {
                        let current_token = *input.connection_token.borrow();
                        if update_token_is_current(event_token, current_token, recovered_transport)
                        {
                            emit_error(
                                &input.events,
                                &input.core_updates,
                                format!("map Telegram update: {error}"),
                                event_token,
                            )
                            .await;
                        }
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

fn update_token_is_current(
    operation_token: Option<ConnectionToken>,
    observed_token: Option<ConnectionToken>,
    recovered_transport: bool,
) -> bool {
    if recovered_transport {
        let recovered_token = operation_token.map(ConnectionToken::next_epoch);
        observed_token == operation_token || observed_token == recovered_token
    } else {
        observed_token == operation_token
    }
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
    reaction_revision: ReactionRevision,
    activity_generations: &mut ActivityGenerations,
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
            if let Some(message) = message_added_if_new(summary, dedup).await {
                events.push(message);
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
        Update::Raw(raw) => match &raw.raw {
            tl::enums::Update::UserTyping(_)
            | tl::enums::Update::ChatUserTyping(_)
            | tl::enums::Update::ChannelUserTyping(_) => {
                Ok(map_raw_activity_update(&raw.raw, activity_generations)
                    .into_iter()
                    .collect())
            }
            tl::enums::Update::MessagePoll(update) => Ok(poll_from_update(update, polls)
                .await?
                .map(TelegramUpdate::PollUpdated)
                .into_iter()
                .collect()),
            tl::enums::Update::MessageReactions(update) => {
                let chat_id = match &update.peer {
                    tl::enums::Peer::User(peer) => peer.user_id,
                    tl::enums::Peer::Chat(peer) => peer.chat_id.saturating_neg(),
                    tl::enums::Peer::Channel(peer) => channel_id_to_core(peer.channel_id),
                };
                Ok(vec![TelegramUpdate::MessageReactionsUpdated {
                    chat_id,
                    message_id: i64::from(update.msg_id),
                    reactions: map_reactions(&update.reactions),
                    revision: reaction_revision,
                }])
            }
            tl::enums::Update::ContactsReset => Ok(vec![TelegramUpdate::ContactsRefreshRequested]),
            tl::enums::Update::DialogUnreadMark(update) => {
                Ok(map_raw_dialog_unread_mark(update).into_iter().collect())
            }
            tl::enums::Update::ReadHistoryInbox(update) => {
                Ok(vec![TelegramUpdate::ReadReceiptUpdated {
                    chat_id: raw_peer_to_core(&update.peer),
                    direction: ReadDirection::Inbox,
                    max_message_id: i64::from(update.max_id),
                    unread_count: Some(u32::try_from(update.still_unread_count).unwrap_or(0)),
                }])
            }
            tl::enums::Update::ReadHistoryOutbox(update) => {
                Ok(vec![TelegramUpdate::ReadReceiptUpdated {
                    chat_id: raw_peer_to_core(&update.peer),
                    direction: ReadDirection::Outbox,
                    max_message_id: i64::from(update.max_id),
                    unread_count: None,
                }])
            }
            tl::enums::Update::ReadChannelInbox(update) => {
                Ok(vec![TelegramUpdate::ReadReceiptUpdated {
                    chat_id: crate::ids::channel_id_to_core(update.channel_id),
                    direction: ReadDirection::Inbox,
                    max_message_id: i64::from(update.max_id),
                    unread_count: Some(u32::try_from(update.still_unread_count).unwrap_or(0)),
                }])
            }
            tl::enums::Update::ReadChannelOutbox(update) => {
                Ok(vec![TelegramUpdate::ReadReceiptUpdated {
                    chat_id: crate::ids::channel_id_to_core(update.channel_id),
                    direction: ReadDirection::Outbox,
                    max_message_id: i64::from(update.max_id),
                    unread_count: None,
                }])
            }
            _ => Ok(map_raw_pinned_update(&raw.raw)
                .map(|event| vec![event])
                .unwrap_or_else(|| vec![TelegramUpdate::Unhandled])),
        },
        _ => Ok(Vec::new()),
    }
}

async fn message_added_if_new(
    message: MessageSummary,
    dedup: &MessageDeduplicator,
) -> Option<TelegramUpdate> {
    dedup
        .should_emit(&message)
        .await
        .then_some(TelegramUpdate::MessageAdded(message))
}

fn map_raw_dialog_unread_mark(
    update: &grammers_client::tl::types::UpdateDialogUnreadMark,
) -> Option<TelegramUpdate> {
    use grammers_client::tl;

    // This is the manual unread flag, not the message unread count; the core
    // reducer keeps those states separate.
    // Monoforum topic marks carry a saved_peer_id and cannot be represented
    // by the current chat-level CoreState. Do not mark the parent chat instead.
    if update.saved_peer_id.is_some() {
        return None;
    }
    let tl::enums::DialogPeer::Peer(dialog_peer) = &update.peer else {
        return None;
    };

    let chat_id = raw_peer_to_core(&dialog_peer.peer);
    (chat_id != 0).then_some(TelegramUpdate::ChatUnreadMarkChanged {
        chat_id,
        unread: update.unread,
    })
}

fn map_raw_pinned_update(update: &grammers_client::tl::enums::Update) -> Option<TelegramUpdate> {
    use grammers_client::tl;
    let (chat_id, message_ids, pinned, pts) = match update {
        tl::enums::Update::PinnedMessages(update) => (
            peer_to_core_id(&update.peer),
            update.messages.iter().copied().map(i64::from).collect(),
            update.pinned,
            update.pts,
        ),
        tl::enums::Update::PinnedChannelMessages(update) => (
            crate::ids::channel_id_to_core(update.channel_id),
            update.messages.iter().copied().map(i64::from).collect(),
            update.pinned,
            update.pts,
        ),
        _ => return None,
    };
    Some(TelegramUpdate::PinnedMessagesChanged {
        chat_id,
        message_ids,
        pinned,
        pts,
    })
}

fn peer_to_core_id(peer: &grammers_client::tl::enums::Peer) -> i64 {
    use grammers_client::tl;
    match peer {
        tl::enums::Peer::User(peer) => peer.user_id,
        tl::enums::Peer::Chat(peer) => -peer.chat_id,
        tl::enums::Peer::Channel(peer) => crate::ids::channel_id_to_core(peer.channel_id),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, future::Future, pin::Pin, sync::Arc, time::Instant};

    use grammers_client::{InvocationError, sender::RpcError};
    use tokio::sync::watch;

    use super::{
        ActivityGenerations, ActivitySignal, MAX_TRACKED_CHAT_ACTIVITIES, NextOutcome,
        RetryBackoff, UPDATE_RETRY_INITIAL_DELAY, UPDATE_RETRY_MAX_DELAY, UpdateSource,
        activity_signal, emit_lifecycle, emit_update, is_transient_update_error,
        map_raw_activity_update, message_added_if_new, next_with_recovery, update_token_is_current,
        wait_for_current_attempt,
    };
    use super::{
        ConnectionState, TelegramUpdate, map_raw_dialog_unread_mark, map_raw_pinned_update,
    };
    use crate::messages::MessageDeduplicator;
    use tg_mobile_core::{
        ChatActivityKind, ChatActivityTracker, ConnectionMachine, CoreEvent,
        DEFAULT_CHAT_ACTIVITY_TTL_MS, LinkEvent, MessageSummary, ReadDirection,
        backend_update_channel,
    };

    fn sample_message(id: i64, chat_id: i64) -> MessageSummary {
        MessageSummary {
            id,
            chat_id,
            sender_id: 7,
            text: "hello".into(),
            timestamp_unix: 1,
            outgoing: false,
            location: None,
            attachments: Vec::new(),
            forwarded_from: None,
            edited: false,
            edit_timestamp_unix: None,
            service_message: false,
            service: None,
            media_caption: None,
            reply_to: None,
            reactions: Vec::new(),
            reaction_revision: None,
        }
    }

    #[test]
    fn activity_actions_map_typing_recording_upload_and_cancel() {
        use grammers_client::tl;

        assert_eq!(
            activity_signal(&tl::enums::SendMessageAction::SendMessageTypingAction),
            ActivitySignal::Started(ChatActivityKind::Typing),
        );
        assert_eq!(
            activity_signal(&tl::enums::SendMessageAction::SendMessageRecordAudioAction),
            ActivitySignal::Started(ChatActivityKind::RecordingVoice),
        );
        assert_eq!(
            activity_signal(&tl::enums::SendMessageAction::SendMessageRecordVideoAction),
            ActivitySignal::Started(ChatActivityKind::RecordingVideo),
        );
        for action in [
            tl::enums::SendMessageAction::SendMessageUploadPhotoAction(
                tl::types::SendMessageUploadPhotoAction { progress: 50 },
            ),
            tl::enums::SendMessageAction::SendMessageUploadVideoAction(
                tl::types::SendMessageUploadVideoAction { progress: 50 },
            ),
            tl::enums::SendMessageAction::SendMessageUploadAudioAction(
                tl::types::SendMessageUploadAudioAction { progress: 50 },
            ),
            tl::enums::SendMessageAction::SendMessageUploadDocumentAction(
                tl::types::SendMessageUploadDocumentAction { progress: 50 },
            ),
        ] {
            assert_eq!(
                activity_signal(&action),
                ActivitySignal::Started(ChatActivityKind::UploadingMedia),
            );
        }
        assert_eq!(
            activity_signal(&tl::enums::SendMessageAction::SendMessageCancelAction),
            ActivitySignal::Ended,
        );
        assert_eq!(
            activity_signal(&tl::enums::SendMessageAction::SendMessageGeoLocationAction),
            ActivitySignal::Ignored,
        );
    }

    #[test]
    fn activity_generations_advance_and_cancel_targets_only_current_activity() {
        let mut generations = ActivityGenerations::default();
        let first = generations
            .update(12, 34, ActivitySignal::Started(ChatActivityKind::Typing))
            .unwrap();
        assert_eq!(
            first.to_core_event(),
            Some(CoreEvent::ChatActivityChanged {
                chat_id: 12,
                user_id: 34,
                generation: 1,
                activity: ChatActivityKind::Typing,
                ttl_ms: tg_mobile_core::DEFAULT_CHAT_ACTIVITY_TTL_MS,
            })
        );
        let ended = generations.update(12, 34, ActivitySignal::Ended).unwrap();
        assert_eq!(
            ended.to_core_event(),
            Some(CoreEvent::ChatActivityEnded {
                chat_id: 12,
                user_id: 34,
                generation: 1,
            })
        );
        assert!(generations.update(12, 34, ActivitySignal::Ended).is_none());
        let second = generations
            .update(
                12,
                34,
                ActivitySignal::Started(ChatActivityKind::UploadingMedia),
            )
            .unwrap();
        assert!(matches!(
            second,
            TelegramUpdate::Typing { generation: 2, .. }
        ));

        let now = std::time::Instant::now();
        let mut tracker = ChatActivityTracker::default();
        tracker.apply(&first.to_core_event().unwrap(), now);
        let second_event = second.to_core_event().unwrap();
        tracker.apply(&second_event, now);
        tracker.apply(&ended.to_core_event().unwrap(), now);
        assert_eq!(
            tracker.activity(12, 34),
            Some(ChatActivityKind::UploadingMedia),
            "a stop from an older generation must not clear newer activity",
        );
        let expired = tracker.expire(now + std::time::Duration::from_secs(7));
        assert!(matches!(
            expired.as_slice(),
            [CoreEvent::ChatActivityExpired { generation: 2, .. }]
        ));
    }

    #[test]
    fn malformed_activity_peer_ids_are_dropped_without_panicking() {
        let mut generations = ActivityGenerations::default();
        assert!(
            generations
                .update(0, 9, ActivitySignal::Started(ChatActivityKind::Typing))
                .is_none()
        );
        assert!(
            generations
                .update(-8, 0, ActivitySignal::Started(ChatActivityKind::Typing))
                .is_none()
        );
        assert!(generations.update(9, -1, ActivitySignal::Ended).is_none());
    }

    #[test]
    fn activity_generation_storage_is_bounded_and_expired_entries_are_pruned() {
        let mut generations = ActivityGenerations::default();
        for user_id in 1..=(MAX_TRACKED_CHAT_ACTIVITIES as i64 + 1) {
            generations
                .update(
                    12,
                    user_id,
                    ActivitySignal::Started(ChatActivityKind::Typing),
                )
                .unwrap();
        }
        assert_eq!(generations.active.len(), MAX_TRACKED_CHAT_ACTIVITIES);

        generations.prune_expired(
            Instant::now() + std::time::Duration::from_millis(DEFAULT_CHAT_ACTIVITY_TTL_MS + 1),
        );
        assert!(generations.active.is_empty());
        let next = generations
            .update(12, 1, ActivitySignal::Started(ChatActivityKind::Typing))
            .unwrap();
        assert!(matches!(
            next,
            TelegramUpdate::Typing {
                generation,
                ..
            } if generation == MAX_TRACKED_CHAT_ACTIVITIES as u64 + 2
        ));
    }

    #[test]
    fn raw_chat_typing_maps_the_chat_and_sender_peer() {
        use grammers_client::tl;

        let update = tl::enums::Update::ChatUserTyping(tl::types::UpdateChatUserTyping {
            chat_id: 44,
            from_id: tl::enums::Peer::User(tl::types::PeerUser { user_id: 77 }),
            action: tl::enums::SendMessageAction::SendMessageTypingAction,
        });
        let mapped = map_raw_activity_update(&update, &mut ActivityGenerations::default()).unwrap();
        assert!(matches!(
            mapped,
            TelegramUpdate::Typing {
                chat_id: -44,
                sender_id: 77,
                generation: 1,
                activity: ChatActivityKind::Typing,
                ..
            }
        ));
    }

    #[test]
    fn raw_user_and_channel_typing_are_mapped_and_invalid_peers_are_ignored() {
        use grammers_client::tl;

        let mut generations = ActivityGenerations::default();
        let user = tl::enums::Update::UserTyping(tl::types::UpdateUserTyping {
            user_id: 77,
            top_msg_id: None,
            action: tl::enums::SendMessageAction::SendMessageRecordAudioAction,
        });
        assert!(matches!(
            map_raw_activity_update(&user, &mut generations),
            Some(TelegramUpdate::Typing {
                chat_id: 77,
                sender_id: 77,
                activity: ChatActivityKind::RecordingVoice,
                ..
            })
        ));

        let channel = tl::enums::Update::ChannelUserTyping(tl::types::UpdateChannelUserTyping {
            channel_id: 44,
            top_msg_id: None,
            from_id: tl::enums::Peer::User(tl::types::PeerUser { user_id: 88 }),
            action: tl::enums::SendMessageAction::SendMessageUploadPhotoAction(
                tl::types::SendMessageUploadPhotoAction { progress: 20 },
            ),
        });
        assert!(matches!(
            map_raw_activity_update(&channel, &mut generations),
            Some(TelegramUpdate::Typing {
                chat_id,
                sender_id: 88,
                activity: ChatActivityKind::UploadingMedia,
                ..
            }) if chat_id == crate::ids::channel_id_to_core(44)
        ));

        let malformed = tl::enums::Update::UserTyping(tl::types::UpdateUserTyping {
            user_id: 0,
            top_msg_id: None,
            action: tl::enums::SendMessageAction::SendMessageTypingAction,
        });
        assert!(map_raw_activity_update(&malformed, &mut generations).is_none());
    }

    #[test]
    fn dialog_unread_mark_updates_only_map_chat_level_peers() {
        use grammers_client::tl;

        let update = tl::types::UpdateDialogUnreadMark {
            unread: true,
            peer: tl::enums::DialogPeer::Peer(tl::types::DialogPeer {
                peer: tl::enums::Peer::User(tl::types::PeerUser { user_id: 77 }),
            }),
            saved_peer_id: None,
        };
        let mapped = map_raw_dialog_unread_mark(&update);
        assert_eq!(
            mapped,
            Some(TelegramUpdate::ChatUnreadMarkChanged {
                chat_id: 77,
                unread: true,
            })
        );
        assert_eq!(
            mapped.and_then(|event| event.to_core_event()),
            Some(CoreEvent::ChatUnreadMarkChanged {
                chat_id: 77,
                unread: true,
            })
        );

        let mut topic_update = update.clone();
        topic_update.saved_peer_id =
            Some(tl::enums::Peer::User(tl::types::PeerUser { user_id: 88 }));
        assert!(map_raw_dialog_unread_mark(&topic_update).is_none());

    }

    #[test]
    fn pinned_mtproto_updates_preserve_peer_message_ids_and_pts() {
        use grammers_client::tl;

        let update = tl::enums::Update::PinnedMessages(tl::types::UpdatePinnedMessages {
            pinned: false,
            peer: tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 42 }),
            messages: vec![3, 8],
            pts: 91,
            pts_count: 2,
        });

        let mapped = map_raw_pinned_update(&update).unwrap();
        assert_eq!(
            mapped,
            TelegramUpdate::PinnedMessagesChanged {
                chat_id: -42,
                message_ids: vec![3, 8],
                pinned: false,
                pts: 91,
            }
        );
        assert_eq!(
            mapped.to_core_event(),
            Some(CoreEvent::PinnedMessagesChanged {
                chat_id: -42,
                message_ids: vec![3, 8],
                pinned: false,
                pts: Some(91),
            })
        );
    }

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
    async fn transport_failure_reconnects_and_first_resumed_update_uses_new_epoch() {
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
        let (token_tx, token_rx) = watch::channel(Some(initial));
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let transient_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let transient_flag = Arc::clone(&transient_seen);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut source = FakeUpdateSource {
            results: VecDeque::from([
                Err(InvocationError::Io(std::io::Error::other("network lost"))),
                Ok(42),
            ]),
            calls,
        };
        let mut backoff = RetryBackoff::default();
        let core_for_retry = core_tx.clone();
        let native_for_retry = native_tx.clone();
        let result = next_with_recovery(&mut source, &mut shutdown_rx, &mut backoff, move |_| {
            let core = Some(core_for_retry.clone());
            let native = native_for_retry.clone();
            let token_tx = token_tx.clone();
            let transient_flag = Arc::clone(&transient_flag);
            async move {
                let recovered = emit_lifecycle(&native, &core, initial, LinkEvent::TransportLost)
                    .await
                    .unwrap();
                token_tx.send_replace(Some(recovered));
                transient_flag.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        })
        .await;

        assert!(matches!(result, NextOutcome::Item(42)));
        assert!(transient_seen.load(std::sync::atomic::Ordering::SeqCst));
        machine.apply(initial, LinkEvent::TransportLost).unwrap();
        assert_eq!(machine.phase(), tg_mobile_core::LinkPhase::Reconnecting);
        let recovered = *token_rx.borrow();
        assert_eq!(recovered, Some(machine.token()));
        assert!(!update_token_is_current(Some(initial), recovered, false));
        assert!(update_token_is_current(Some(initial), recovered, true));
        let resumed_token = Some(initial.next_epoch());
        assert_eq!(recovered, resumed_token);

        let after_retry = emit_lifecycle(
            &native_tx,
            &Some(core_tx.clone()),
            recovered.unwrap(),
            LinkEvent::Retry,
        )
        .await
        .unwrap();
        let retry_token = machine.token();
        machine.apply(retry_token, LinkEvent::Retry).unwrap();
        let after_connect = emit_lifecycle(
            &native_tx,
            &Some(core_tx.clone()),
            after_retry,
            LinkEvent::TransportConnected,
        )
        .await
        .unwrap();
        let connect_token = machine.token();
        machine
            .apply(connect_token, LinkEvent::TransportConnected)
            .unwrap();
        emit_lifecycle(
            &native_tx,
            &Some(core_tx.clone()),
            after_connect,
            LinkEvent::SyncStarted,
        )
        .await;
        let sync_token = machine.token();
        machine.apply(sync_token, LinkEvent::SyncStarted).unwrap();

        while core_rx.try_recv().is_ok() {}
        let event_token = initial.next_epoch();
        emit_update(
            &native_tx,
            &Some(core_tx),
            TelegramUpdate::MessageAdded(sample_message(42, 9)),
            Some(event_token),
        )
        .await;
        assert_eq!(
            core_rx.recv().await.unwrap().unwrap(),
            CoreEvent::SessionUpdate {
                token: event_token,
                event: Box::new(CoreEvent::MessageAdded(sample_message(42, 9))),
            }
        );
        let _ = shutdown_tx.send(true);
    }

    #[tokio::test]
    async fn duplicate_incoming_message_is_suppressed_across_replayed_updates() {
        let dedup = MessageDeduplicator::new(8);
        let message = sample_message(123, -4);
        let first = message_added_if_new(message.clone(), &dedup).await;
        let replay = message_added_if_new(message.clone(), &dedup).await;
        let same_id_other_chat = message_added_if_new(sample_message(123, -5), &dedup).await;

        assert_eq!(first, Some(TelegramUpdate::MessageAdded(message)));
        assert_eq!(replay, None);
        assert!(matches!(
            same_id_other_chat,
            Some(TelegramUpdate::MessageAdded(MessageSummary {
                chat_id: -5,
                ..
            }))
        ));
    }

    #[test]
    fn updates_from_a_replaced_session_generation_are_stale() {
        let mut machine = ConnectionMachine::default();
        machine.start_session();
        let old = machine.token();
        machine.apply(old, LinkEvent::Logout).unwrap();
        machine.start_session();
        let new = machine.token();

        assert_ne!(old.generation(), new.generation());
        assert!(!update_token_is_current(Some(old), Some(new), false));
        assert!(!update_token_is_current(Some(old), Some(new), true));
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

    struct PendingUpdateSource {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl UpdateSource for PendingUpdateSource {
        type Item = u8;

        fn next(
            &mut self,
        ) -> Pin<Box<dyn Future<Output = Result<Self::Item, InvocationError>> + Send + '_>>
        {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(std::future::pending())
        }
    }

    #[tokio::test]
    async fn shutdown_cancels_a_pending_stream_read() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut source = PendingUpdateSource {
            calls: Arc::clone(&calls),
        };
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut backoff = RetryBackoff::default();
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
            location: None,
            attachments: Vec::new(),
            forwarded_from: None,
            edited: false,
            edit_timestamp_unix: None,
            service_message: false,
            service: None,
            media_caption: None,
            reply_to: None,
            reactions: Vec::new(),
            reaction_revision: None,
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
            TelegramUpdate::ReadReceiptUpdated {
                chat_id: 77,
                direction: ReadDirection::Inbox,
                max_message_id: 42,
                unread_count: Some(5),
            }
            .to_core_event(),
            Some(CoreEvent::ReadReceiptUpdated {
                chat_id: 77,
                direction: ReadDirection::Inbox,
                max_message_id: 42,
                unread_count: Some(5),
            })
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
        assert_eq!(
            TelegramUpdate::ContactsRefreshRequested.to_core_event(),
            Some(CoreEvent::ContactsRefreshRequested),
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
            location: None,
            attachments: Vec::new(),
            forwarded_from: None,
            edited: false,
            edit_timestamp_unix: None,
            service_message: false,
            service: None,
            media_caption: None,
            reply_to: None,

            reactions: Vec::new(),
            reaction_revision: None,
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
    async fn typing_activity_is_delivered_through_the_session_scoped_core_path() {
        let mut machine = ConnectionMachine::default();
        machine.start_session();
        let token = machine.token();
        let (core_tx, mut core_rx) = backend_update_channel();
        let (native_tx, _) = tokio::sync::broadcast::channel(1);

        super::emit_update(
            &native_tx,
            &Some(core_tx),
            TelegramUpdate::Typing {
                chat_id: -42,
                sender_id: 7,
                generation: 3,
                activity: ChatActivityKind::Typing,
                ttl_ms: 6_000,
            },
            Some(token),
        )
        .await;

        assert_eq!(
            core_rx.recv().await.unwrap().unwrap(),
            CoreEvent::SessionUpdate {
                token,
                event: Box::new(CoreEvent::ChatActivityChanged {
                    chat_id: -42,
                    user_id: 7,
                    generation: 3,
                    activity: ChatActivityKind::Typing,
                    ttl_ms: 6_000,
                }),
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
