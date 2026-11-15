use serde::{Deserialize, Serialize};

use crate::{
    connection::ConnectionState,
    model::{AccountState, AuthorizationState, ChatSummary, MessageSummary},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreEvent {
    State(AccountState),
    AuthorizationChanged(AuthorizationState),
    ConnectionStateChanged(ConnectionState),
    ChatsLoaded(Vec<ChatSummary>),
    MessagesLoaded {
        chat_id: i64,
        messages: Vec<MessageSummary>,
    },
    ChatRead {
        chat_id: i64,
        max_message_id: i64,
    },
    MessageAdded(MessageSummary),
    Error(String),
    Stopped,
}

impl CoreEvent {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::State(_) => "state",
            Self::AuthorizationChanged(_) => "authorization_changed",
            Self::ConnectionStateChanged(_) => "connection_state_changed",
            Self::ChatsLoaded(_) => "chats_loaded",
            Self::MessagesLoaded { .. } => "messages_loaded",
            Self::ChatRead { .. } => "chat_read",
            Self::MessageAdded(_) => "message_added",
            Self::Error(_) => "error",
            Self::Stopped => "stopped",
        }
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}
