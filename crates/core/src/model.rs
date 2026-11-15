use serde::{Deserialize, Serialize};

/// Telegram activities surfaced transiently while a user is composing or uploading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatActivityKind {
    Typing,
    RecordingVoice,
    UploadingMedia,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorizationState {
    WaitingForPhoneNumber,
    WaitingForCode,
    WaitingForPassword,
    Ready,
    LoggingOut,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSummary {
    pub id: i64,
    pub title: String,
    pub unread_count: u32,
    pub last_message: Option<MessageSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: i64,
    pub chat_id: i64,
    pub sender_id: i64,
    pub text: String,
    pub timestamp_unix: i64,
    pub outgoing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    pub authorization: AuthorizationState,
    pub chats: Vec<ChatSummary>,
}

impl Default for AccountState {
    fn default() -> Self {
        Self {
            authorization: AuthorizationState::Closed,
            chats: Vec::new(),
        }
    }
}
