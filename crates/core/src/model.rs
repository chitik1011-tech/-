use serde::{Deserialize, Serialize};

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

/// One cursor-addressed page of chat summaries.
///
/// `requested_cursor` is opaque and must be the cursor used to request this
/// page. `None` denotes the first page. The backend returns the next opaque
/// cursor, or `None` when pagination is complete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPage {
    pub requested_cursor: Option<String>,
    pub chats: Vec<ChatSummary>,
    pub next_cursor: Option<String>,
}

/// Core-owned progress for merging cursor-addressed chat pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChatPaginationState {
    /// Whether a first page has been accepted.
    pub loaded: bool,
    /// Cursor to pass to the next `LoadChatsPage` command.
    pub next_cursor: Option<String>,
    /// Cursors already consumed in this page chain, used to stop cursor loops.
    #[serde(default)]
    pub consumed_cursors: std::collections::BTreeSet<String>,
    /// Whether a first or next page request is currently running.
    #[serde(default)]
    pub loading: bool,
    /// Cursor for the in-flight request; `None` means the first page.
    #[serde(default)]
    pub loading_cursor: Option<String>,
    /// Latest page-specific error, retained for retry and UI display.
    #[serde(default)]
    pub error: Option<String>,
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
