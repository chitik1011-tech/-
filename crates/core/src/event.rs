use crate::model::{AccountState, AuthorizationState, ChatSummary, MessageSummary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEvent {
    State(AccountState),
    AuthorizationChanged(AuthorizationState),
    ChatsLoaded(Vec<ChatSummary>),
    MessageAdded(MessageSummary),
    Error(String),
    Stopped,
}
