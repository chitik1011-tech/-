use serde::{Deserialize, Serialize};

use crate::model::{AccountState, AuthorizationState, ChatPage, ChatSummary, MessageSummary};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreEvent {
    State(AccountState),
    AuthorizationChanged(AuthorizationState),
    ChatsLoaded(Vec<ChatSummary>),
    /// Signals that a first or next page request has started.
    ChatsPageLoading { cursor: Option<String> },
    /// A cursor-addressed chat page. Stale page responses are ignored by CoreState.
    ChatsPageLoaded(ChatPage),
    /// A failed page request keeps its cursor available for retry.
    ChatsPageFailed { cursor: Option<String>, error: String },
    MessageAdded(MessageSummary),
    Error(String),
    Stopped,
}

impl CoreEvent {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::State(_) => "state",
            Self::AuthorizationChanged(_) => "authorization_changed",
            Self::ChatsLoaded(_) => "chats_loaded",
            Self::ChatsPageLoading { .. } => "chats_page_loading",
            Self::ChatsPageLoaded(_) => "chats_page_loaded",
            Self::ChatsPageFailed { .. } => "chats_page_failed",
            Self::MessageAdded(_) => "message_added",
            Self::Error(_) => "error",
            Self::Stopped => "stopped",
        }
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}
