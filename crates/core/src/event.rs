use serde::{Deserialize, Serialize};

use crate::{
    contacts::Contact,
    model::{AccountState, AuthorizationState, ChatSummary, MessageSummary},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreEvent {
    State(AccountState),
    AuthorizationChanged(AuthorizationState),
    ChatsLoaded(Vec<ChatSummary>),
    MessageAdded(MessageSummary),
    ContactAdded(Contact),
    ContactDeleted {
        user_id: i64,
    },
    ContactsLoaded {
        offset: usize,
        total: usize,
        contacts: Vec<Contact>,
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
            Self::ContactAdded(_) => "contact_added",
            Self::ContactDeleted { .. } => "contact_deleted",
            Self::ContactsLoaded { .. } => "contacts_loaded",
            Self::Error(_) => "error",
            Self::Stopped => "stopped",
        }
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}
