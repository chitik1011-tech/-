use serde::{Deserialize, Serialize};

use crate::model::{AccountState, AuthorizationState, ChatSummary, MessageCursor, MessageSummary};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreEvent {
    State(AccountState),
    AuthorizationChanged(AuthorizationState),
    ChatsLoaded(Vec<ChatSummary>),
    MessagesLoading { chat_id: i64, cursor: Option<MessageCursor> },
    MessagesLoaded { chat_id: i64, cursor: Option<MessageCursor>, messages: Vec<MessageSummary>, next_cursor: Option<MessageCursor>, has_more: bool },
    MessageHistoryLoadFailed { chat_id: i64, cursor: Option<MessageCursor>, error: String },
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
            Self::MessagesLoading { .. } => "messages_loading",
            Self::MessagesLoaded { .. } => "messages_loaded",
            Self::MessageHistoryLoadFailed { .. } => "message_history_load_failed",
            Self::MessageAdded(_) => "message_added",
            Self::Error(_) => "error",
            Self::Stopped => "stopped",
        }
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}
