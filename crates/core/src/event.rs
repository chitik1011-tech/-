use serde::{Deserialize, Serialize};

use crate::{
    forwarding::ForwardMessagesResult,
    model::{AccountState, AuthorizationState, ChatSummary, MessageSummary},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreEvent {
    State(AccountState),
    AuthorizationChanged(AuthorizationState),
    ChatsLoaded(Vec<ChatSummary>),
    MessageAdded(MessageSummary),
    MessagesForwarded(ForwardMessagesResult),
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
            Self::MessagesForwarded(_) => "messages_forwarded",
            Self::Error(_) => "error",
            Self::Stopped => "stopped",
        }
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}
