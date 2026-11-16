use serde::{Deserialize, Serialize};

use crate::{
    connection::ConnectionState,
    connection_machine::{ConnectionToken, LinkEvent},
    forwarding::ForwardMessagesResult,
    model::{
        AccountState, AttachmentDownloadState, AuthorizationState, ChatActivityKind, ChatSummary,
        MessageReaction, MessageSummary, ReactionRevision,
    },
    peer::PeerResolution,
    poll::PollSummary,
    search::{MessageSearchPage, SearchMessagesRequest},
    send::OutgoingSend,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreEvent {
    State(AccountState),
    AuthorizationChanged(AuthorizationState),
    /// Starts a new connection session and creates its generation token.
    ConnectionSessionStarted,
    /// Lifecycle input produced by work started for a specific connection token.
    ConnectionLifecycle {
        token: ConnectionToken,
        event: LinkEvent,
    },
    /// Update produced by asynchronous work. The actor drops it if its token is stale.
    SessionUpdate {
        token: ConnectionToken,
        event: Box<CoreEvent>,
    },
    /// Legacy untagged state event. Runtime reducers ignore it; use `ConnectionLifecycle`.
    ConnectionStateChanged(ConnectionState),
    ChatsLoaded(Vec<ChatSummary>),
    MessagesLoaded {
        chat_id: i64,
        messages: Vec<MessageSummary>,
    },
    MessagesPageLoading {
        chat_id: i64,
        before_message_id: Option<i64>,
    },
    MessagesPageLoaded {
        chat_id: i64,
        before_message_id: Option<i64>,
        messages: Vec<MessageSummary>,
        next_before_message_id: Option<i64>,
        has_more: bool,
    },
    MessagesPageLoadFailed {
        chat_id: i64,
        before_message_id: Option<i64>,
        error: String,
    },
    ChatRead {
        chat_id: i64,
        max_message_id: i64,
    },
    /// Monotonic Telegram read boundary for one side of a chat.
    ReadReceiptUpdated {
        chat_id: i64,
        direction: crate::model::ReadDirection,
        max_message_id: i64,
    },
    MessageSearchStarted(SearchMessagesRequest),
    MessageSearchCompleted(MessageSearchPage),
    MessageSearchFailed {
        request_id: u64,
        chat_id: i64,
        query: String,
        before_message_id: Option<i64>,
        error: String,
    },
    MessageAdded(MessageSummary),
    ChatArchived {
        chat_id: i64,
        archived: bool,
    },
    ChatMuted {
        chat_id: i64,
        muted: bool,
    },
    ChatPinned {
        chat_id: i64,
        pinned: bool,
    },
    ChatUnreadChanged {
        chat_id: i64,
        unread: bool,
    },
    MessageEdited(MessageSummary),
    MediaTransferUpdated {
        chat_id: i64,
        message_id: i64,
        attachment_index: usize,
        #[serde(default)]
        generation: u64,
        state: AttachmentDownloadState,
    },
    MessagesDeleted {
        chat_id: Option<i64>,
        message_ids: Vec<i64>,
    },
    /// Poll snapshot for a message. Replaces the stored snapshot with the same poll id.
    PollUpdated(PollSummary),
    /// The Telegram vote RPC completed successfully. Final choice/results still come from PollUpdated.
    PollVoteSubmitted {
        poll_id: i64,
    },
    /// The Telegram vote RPC failed without replacing the last server-confirmed poll snapshot.
    PollVoteFailed {
        poll_id: i64,
        error: String,
    },
    /// Authoritative Telegram reaction state for a cached message.
    MessageReactionsUpdated {
        chat_id: i64,
        message_id: i64,
        reactions: Vec<MessageReaction>,
        revision: ReactionRevision,
    },
    /// Finishes the UI's pending indicator after the reaction RPC completes.
    ReactionOperationSucceeded {
        chat_id: i64,
        message_id: i64,
    },
    /// Reports a reaction RPC failure without losing the last server snapshot.
    ReactionOperationFailed {
        chat_id: i64,
        message_id: i64,
        error: String,
    },
    /// Forward results; successful messages also arrive as `MessageAdded`.
    MessagesForwarded(ForwardMessagesResult),
    MessageSendStateChanged(OutgoingSend),
    /// `generation` must monotonically increase per (chat_id, user_id).
    ChatActivityChanged {
        chat_id: i64,
        user_id: i64,
        generation: u64,
        activity: ChatActivityKind,
        ttl_ms: u64,
    },
    ChatActivityEnded {
        chat_id: i64,
        user_id: i64,
        generation: u64,
    },
    ChatActivityExpired {
        chat_id: i64,
        user_id: i64,
        generation: u64,
    },
    DraftUpdated(crate::model::Draft),
    DraftCleared {
        chat_id: i64,
        expected_updated_at_unix: Option<i64>,
        #[serde(default)]
        expected_draft: Option<crate::model::Draft>,
    },
    PeerResolved(PeerResolution),
    Error(String),
    Stopped,
}

impl CoreEvent {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::State(_) => "state",
            Self::AuthorizationChanged(_) => "authorization_changed",
            Self::ConnectionSessionStarted => "connection_session_started",
            Self::ConnectionLifecycle { .. } => "connection_lifecycle",
            Self::SessionUpdate { .. } => "session_update",
            Self::ConnectionStateChanged(_) => "connection_state_changed",
            Self::ChatsLoaded(_) => "chats_loaded",
            Self::MessagesLoaded { .. } => "messages_loaded",
            Self::MessagesPageLoading { .. } => "messages_page_loading",
            Self::MessagesPageLoaded { .. } => "messages_page_loaded",
            Self::MessagesPageLoadFailed { .. } => "messages_page_load_failed",
            Self::ChatRead { .. } => "chat_read",
            Self::ReadReceiptUpdated { .. } => "read_receipt_updated",
            Self::MessageSearchStarted(_) => "message_search_started",
            Self::MessageSearchCompleted(_) => "message_search_completed",
            Self::MessageSearchFailed { .. } => "message_search_failed",
            Self::MessageAdded(_) => "message_added",
            Self::ChatArchived { .. } => "chat_archived",
            Self::ChatMuted { .. } => "chat_muted",
            Self::ChatPinned { .. } => "chat_pinned",
            Self::ChatUnreadChanged { .. } => "chat_unread_changed",
            Self::MessageEdited(_) => "message_edited",
            Self::MediaTransferUpdated { .. } => "media_transfer_updated",
            Self::MessagesDeleted { .. } => "messages_deleted",
            Self::PollUpdated(_) => "poll_updated",
            Self::PollVoteSubmitted { .. } => "poll_vote_submitted",
            Self::PollVoteFailed { .. } => "poll_vote_failed",
            Self::MessageReactionsUpdated { .. } => "message_reactions_updated",
            Self::ReactionOperationSucceeded { .. } => "reaction_operation_succeeded",
            Self::ReactionOperationFailed { .. } => "reaction_operation_failed",
            Self::MessagesForwarded(_) => "messages_forwarded",
            Self::MessageSendStateChanged(_) => "message_send_state_changed",
            Self::ChatActivityChanged { .. } => "chat_activity_changed",
            Self::ChatActivityEnded { .. } => "chat_activity_ended",
            Self::ChatActivityExpired { .. } => "chat_activity_expired",
            Self::DraftUpdated(_) => "draft_updated",
            Self::DraftCleared { .. } => "draft_cleared",
            Self::PeerResolved(_) => "peer_resolved",
            Self::Error(_) => "error",
            Self::Stopped => "stopped",
        }
    }

    pub const fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::PollVoteSubmitted { .. }
                | Self::PollVoteFailed { .. }
                | Self::MessagesForwarded(_)
                | Self::ChatActivityChanged { .. }
                | Self::ChatActivityEnded { .. }
                | Self::ChatActivityExpired { .. }
                | Self::ReactionOperationSucceeded { .. }
                | Self::ReactionOperationFailed { .. }
        )
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}
