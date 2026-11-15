use serde::{Deserialize, Serialize};

use crate::{
    connection::ConnectionState,
    connection_machine::{ConnectionToken, LinkEvent},
    model::{AccountState, AuthorizationState, ChatSummary, MessageSummary},
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
    MessageAdded(MessageSummary),
    MessageEdited(MessageSummary),
    MessagesDeleted {
        chat_id: Option<i64>,
        message_ids: Vec<i64>,
    },
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
            Self::MessageAdded(_) => "message_added",
            Self::MessageEdited(_) => "message_edited",
            Self::MessagesDeleted { .. } => "messages_deleted",
            Self::Error(_) => "error",
            Self::Stopped => "stopped",
        }
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}
