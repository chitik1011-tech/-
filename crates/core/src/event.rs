use serde::{Deserialize, Serialize};

use crate::model::{
    AccountState, AuthorizationState, ChatSummary, MessageReaction, MessageSummary,
    ReactionOperation,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreEvent {
    State(AccountState),
    AuthorizationChanged(AuthorizationState),
    ChatsLoaded(Vec<ChatSummary>),
    MessageAdded(MessageSummary),
    /// Replaces the complete current reaction list for one cached message.
    MessageReactionsUpdated {
        chat_id: i64,
        message_id: i64,
        reactions: Vec<MessageReaction>,
    },
    ReactionOperationSucceeded {
        chat_id: i64,
        message_id: i64,
        emoji: String,
        operation: ReactionOperation,
    },
    ReactionOperationFailed {
        chat_id: i64,
        message_id: i64,
        emoji: String,
        operation: ReactionOperation,
        error: String,
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
            Self::MessageReactionsUpdated { .. } => "message_reactions_updated",
            Self::ReactionOperationSucceeded { .. } => "reaction_operation_succeeded",
            Self::ReactionOperationFailed { .. } => "reaction_operation_failed",
            Self::Error(_) => "error",
            Self::Stopped => "stopped",
        }
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}
