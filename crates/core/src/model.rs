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

/// Per-chat unread counters and user-visible chat flags.
///
/// `unread_messages` mirrors the legacy `ChatSummary::unread_count` field for
/// wire/API compatibility. Mentions and reactions are independent counters:
/// updating the message count does not implicitly clear either one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatUnreadState {
    pub unread_messages: u32,
    pub unread_mentions: u32,
    pub unread_reactions: u32,
    pub muted: bool,
    pub unread_mark: bool,
}

/// Partial, absolute update from the backend.
///
/// A `None` field means Telegram did not report that value in this update, so
/// the reducer preserves the previously known value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatUnreadUpdate {
    pub chat_id: i64,
    pub unread_messages: Option<u32>,
    pub unread_mentions: Option<u32>,
    pub unread_reactions: Option<u32>,
    pub muted: Option<bool>,
    pub unread_mark: Option<bool>,
}

/// Aggregate unread counters used for app and navigation badges.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnreadTotals {
    pub unread_messages: u64,
    pub unread_mentions: u64,
    pub unread_reactions: u64,
    pub unread_chats: u64,
    pub muted_chats: u64,
    pub marked_unread_chats: u64,
    /// Message-count badge. Mentions/reactions can refer to already-counted
    /// messages, so they are reported separately and are not added again.
    pub badge_count: u64,
}
