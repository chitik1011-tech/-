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

/// Telegram peer category. Unknown is used until the backend identifies a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PeerKind {
    #[default]
    Unknown,
    User,
    BasicGroup,
    Supergroup,
    Channel,
    Bot,
}

/// Presence is recorded only when Telegram supplies it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PeerPresence {
    #[default]
    Unknown,
    Online,
    Offline { last_seen_unix: i64 },
}

/// Backend-owned avatar/file reference metadata; the core does not download it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvatarReference {
    pub file_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_hash: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_center_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Profile fields are optional so the contract never invents Telegram data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerProfile {
    /// Stable Telegram peer ID. Zero is reserved as invalid.
    pub peer_id: i64,
    pub kind: PeerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Set only when the Telegram backend actually provides a phone number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_bot: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_scam: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_fake: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<AvatarReference>,
    #[serde(default)]
    pub presence: PeerPresence,
}

impl PeerProfile {
    /// Create a placeholder without assuming any Telegram profile details.
    pub fn unknown(peer_id: i64) -> Self {
        Self {
            peer_id,
            kind: PeerKind::Unknown,
            display_name: None,
            first_name: None,
            last_name: None,
            username: None,
            phone: None,
            is_bot: None,
            is_verified: None,
            is_scam: None,
            is_fake: None,
            member_count: None,
            bio: None,
            avatar: None,
            presence: PeerPresence::Unknown,
        }
    }
}

impl Default for AccountState {
    fn default() -> Self {
        Self {
            authorization: AuthorizationState::Closed,
            chats: Vec::new(),
        }
    }
}
