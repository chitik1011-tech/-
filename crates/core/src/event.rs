use serde::{Deserialize, Serialize};

use crate::model::{
    AccountState, AuthorizationState, ChatActivityKind, ChatSummary, MessageSummary,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreEvent {
    State(AccountState),
    AuthorizationChanged(AuthorizationState),
    ChatsLoaded(Vec<ChatSummary>),
    MessageAdded(MessageSummary),
    ChatActivityChanged {
        chat_id: i64,
        user_id: i64,
        activity: ChatActivityKind,
        ttl_ms: u64,
    },
    ChatActivityEnded {
        chat_id: i64,
        user_id: i64,
    },
    ChatActivityExpired {
        chat_id: i64,
        user_id: i64,
    },
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
            Self::ChatActivityChanged { .. } => "chat_activity_changed",
            Self::ChatActivityEnded { .. } => "chat_activity_ended",
            Self::ChatActivityExpired { .. } => "chat_activity_expired",
            Self::Error(_) => "error",
            Self::Stopped => "stopped",
        }
    }

    pub const fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::ChatActivityChanged { .. }
                | Self::ChatActivityEnded { .. }
                | Self::ChatActivityExpired { .. }
        )
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}
