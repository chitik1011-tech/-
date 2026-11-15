use std::path::PathBuf;

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
