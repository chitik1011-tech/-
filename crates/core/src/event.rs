use serde::{Deserialize, Serialize};

use crate::model::{AccountState, AuthorizationState, ChatSummary, MessageSummary};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreEvent {
    State(AccountState),
    AuthorizationChanged(AuthorizationState),
    ChatsLoaded(Vec<ChatSummary>),
    MessageAdded(MessageSummary),
    /// Carries the current content and metadata for an edited message.
    MessageEdited(MessageSummary),
    /// Identifies a deleted message using its chat-local message ID.
    MessageDeleted { chat_id: i64, message_id: i64 },
    Error(String),
    Stopped,
}

impl CoreEvent {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::State(_) => "state",
            Self::AuthorizationChanged(_) => "authorization_changed",
            Self::ChatsLoaded(_) => "chats_loaded",
            Self::MessageAdded(_) => "message_added",
            Self::MessageEdited(_) => "message_edited",
            Self::MessageDeleted { .. } => "message_deleted",
            Self::Error(_) => "error",
            Self::Stopped => "stopped",
        }
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}
