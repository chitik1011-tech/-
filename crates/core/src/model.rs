use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
    /// Whether this chat is archived in the Telegram chat list.
    #[serde(default)]
    pub archived: Option<bool>,
    /// Whether notifications for this chat are muted.
    #[serde(default)]
    pub muted: Option<bool>,
    /// Whether this chat (not an individual message) is pinned in the chat list.
    #[serde(default)]
    pub pinned: Option<bool>,
    /// Whether the chat is marked unread, independently of its unread message count.
    #[serde(default)]
    pub is_unread: Option<bool>,
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
    /// Media attached to this message. Missing fields in older persisted messages
    /// are treated as an empty list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<MediaAttachment>,
    /// Original source metadata when Telegram marks the message as forwarded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forwarded_from: Option<ForwardedFrom>,
    /// Whether Telegram reports that this message has been edited.
    #[serde(default, skip_serializing_if = "is_false")]
    pub edited: bool,
    /// Unix timestamp of the most recent edit, when supplied by Telegram.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_timestamp_unix: Option<i64>,
    /// Whether this message represents a Telegram service action.
    #[serde(default, skip_serializing_if = "is_false")]
    pub service_message: bool,
    /// Caption attached to media, kept separate from ordinary message text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_caption: Option<String>,
    /// Target metadata when this message replies to another message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyMetadata>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ForwardedFrom {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_unix: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyMetadata {
    pub message_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_display_name: Option<String>,
}

/// Telegram media categories represented independently of the UI or downloader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Photo,
    Video,
    Document,
    Audio,
    Voice,
    Animation,
    Sticker,
    VideoNote,
}

/// Metadata for an image preview associated with an attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentThumbnail {
    /// Opaque Telegram file reference for retrieving this thumbnail.
    pub file_reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

/// A validated pair of positive media dimensions, suitable for display math.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaDimensions {
    width: u32,
    height: u32,
}

impl MediaDimensions {
    /// Normalizes dimensions reported by Telegram; zero or negative values are unavailable.
    pub fn from_telegram(width: i32, height: i32) -> Option<Self> {
        let width = u32::try_from(width).ok()?;
        let height = u32::try_from(height).ok()?;
        Self::new(width, height)
    }

    /// Creates dimensions from stored values, rejecting zero and non-Telegram ranges.
    pub fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return None;
        }
        Some(Self { width, height })
    }

    pub const fn width(self) -> u32 {
        self.width
    }

    pub const fn height(self) -> u32 {
        self.height
    }
}

impl AttachmentThumbnail {
    /// Returns a normalized size only when both thumbnail dimensions are valid.
    pub fn dimensions(&self) -> Option<MediaDimensions> {
        MediaDimensions::new(self.width?, self.height?)
    }
}

/// State of a media download. A local path exists only in the Downloaded
/// variant, which represents bytes successfully written to local storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AttachmentDownloadState {
    #[default]
    NotDownloaded,
    Downloading,
    Downloaded {
        local_path: PathBuf,
    },
    Failed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// Backend-neutral metadata for one Telegram attachment.
///
/// `file_reference` is opaque and must be retained as supplied by Telegram.
/// Durations are stored in milliseconds to avoid floating-point serialization
/// and equality issues. No local path is exposed until the download state is
/// `Downloaded`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaAttachment {
    pub kind: AttachmentKind,
    pub file_reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<AttachmentThumbnail>,
    #[serde(default)]
    pub download_state: AttachmentDownloadState,
}

impl MediaAttachment {
    /// Returns a normalized size only when both attachment dimensions are valid.
    pub fn dimensions(&self) -> Option<MediaDimensions> {
        let width = i32::try_from(self.width?).ok()?;
        let height = i32::try_from(self.height?).ok()?;
        MediaDimensions::from_telegram(width, height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageHistoryState {
    pub loading: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_before_message_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_before_message_id: Option<i64>,
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Default for MessageHistoryState {
    fn default() -> Self {
        Self {
            loading: false,
            requested_before_message_id: None,
            next_before_message_id: None,
            has_more: true,
            error: None,
        }
    }
}

/// Text being composed in a chat, including an optional reply or edit target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub chat_id: i64,
    pub text: String,
    pub reply_to_message_id: Option<i64>,
    pub edit_message_id: Option<i64>,
    pub updated_at_unix: i64,
}

impl Draft {
    /// Validate invariants for drafts received over the command wire or loaded from storage.
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::CoreError;

        if self.chat_id == 0 {
            return Err(CoreError::InvalidInput("chat id must be non-zero".into()));
        }
        if self.text.trim().is_empty() {
            return Err(CoreError::InvalidInput("draft text is empty".into()));
        }
        if self.reply_to_message_id.is_some_and(|id| id <= 0)
            || self.edit_message_id.is_some_and(|id| id <= 0)
        {
            return Err(CoreError::InvalidInput(
                "draft target message id must be positive".into(),
            ));
        }
        if self.reply_to_message_id.is_some() && self.edit_message_id.is_some() {
            return Err(CoreError::InvalidInput(
                "draft cannot reply to and edit a message at the same time".into(),
            ));
        }
        if self.updated_at_unix < 0 {
            return Err(CoreError::InvalidInput(
                "draft timestamp must be non-negative".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    pub authorization: AuthorizationState,
    pub chats: Vec<ChatSummary>,
}

/// A saved chat list with deterministic membership and user-defined ordering.
///
/// `include_muted`, `include_read`, and `include_archived` allow matching chats in
/// those states through the automatic category filters. Explicitly included chats
/// always remain included, while explicitly excluded chats always remain excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatFolder {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub included_chat_ids: BTreeSet<i64>,
    #[serde(default)]
    pub excluded_chat_ids: BTreeSet<i64>,
    #[serde(default)]
    pub include_contacts: bool,
    #[serde(default)]
    pub include_non_contacts: bool,
    #[serde(default)]
    pub include_groups: bool,
    #[serde(default)]
    pub include_channels: bool,
    #[serde(default)]
    pub include_bots: bool,
    #[serde(default)]
    pub include_muted: bool,
    #[serde(default)]
    pub include_read: bool,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub position: u32,
}

impl ChatFolder {
    /// Check folder identifiers, title, membership sets, and ordering position.
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::CoreError;

        if self.id <= 0 {
            return Err(CoreError::InvalidInput("folder id must be positive".into()));
        }
        let title = self.title.trim();
        if title.is_empty() {
            return Err(CoreError::InvalidInput("folder title is empty".into()));
        }
        if title.chars().count() > 64 || title.chars().any(char::is_control) {
            return Err(CoreError::InvalidInput(
                "folder title must be at most 64 characters and contain no control characters"
                    .into(),
            ));
        }
        if self
            .included_chat_ids
            .iter()
            .chain(self.excluded_chat_ids.iter())
            .any(|id| *id == 0)
        {
            return Err(CoreError::InvalidInput(
                "folder chat ids must be non-zero".into(),
            ));
        }
        Ok(())
    }

    /// Whether adapter-supplied chat facts satisfy this folder's filters.
    pub fn matches(&self, chat: &ChatFolderFacts) -> bool {
        if self.excluded_chat_ids.contains(&chat.id) {
            return false;
        }
        if self.included_chat_ids.contains(&chat.id) {
            return true;
        }

        if (chat.muted.unwrap_or(false) && !self.include_muted)
            || (chat.read && !self.include_read)
            || (chat.archived.unwrap_or(false) && !self.include_archived)
        {
            return false;
        }

        (chat.is_user && chat.is_contact && self.include_contacts)
            || (chat.is_user && !chat.is_contact && self.include_non_contacts)
            || (chat.is_group && self.include_groups)
            || (chat.is_channel && self.include_channels)
            || (chat.is_bot && self.include_bots)
    }
}
/// Backend-neutral facts used to evaluate a folder without inventing chat metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatFolderFacts {
    pub id: i64,
    pub is_user: bool,
    pub is_contact: bool,
    pub is_group: bool,
    pub is_channel: bool,
    pub is_bot: bool,
    pub muted: Option<bool>,
    pub read: bool,
    pub archived: Option<bool>,
}

impl Default for AccountState {
    fn default() -> Self {
        Self {
            authorization: AuthorizationState::Closed,
            chats: Vec::new(),
        }
    }
}
