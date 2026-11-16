use std::path::PathBuf;

use crate::service_message::ServiceMessage;
use serde::{de, Deserialize, Deserializer, Serialize};
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

/// Which message stream a Telegram read boundary applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadDirection {
    Inbox,
    Outbox,
}

/// Highest message IDs reported as read in each direction for a chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReadReceipt {
    #[serde(default)]
    pub inbox_max_message_id: i64,
    #[serde(default)]
    pub outbox_max_message_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: i64,
    pub chat_id: i64,
    pub sender_id: i64,
    pub text: String,
    pub timestamp_unix: i64,
    pub outgoing: bool,
    /// Optional Telegram location or venue attached to this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<MessageLocation>,
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
    /// Typed Telegram action metadata. New consumers should use this field; the
    /// boolean above remains for compatibility with older persisted state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceMessage>,
    /// Caption attached to media, kept separate from ordinary message text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_caption: Option<String>,
    /// Target metadata when this message replies to another message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyMetadata>,
    /// Telegram reaction counts for this message. Older persisted messages have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<MessageReaction>,
    /// Last accepted reaction snapshot ordering token. Telegram updates are delivered
    /// in protocol order; this also rejects callbacks from older connection epochs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction_revision: Option<ReactionRevision>,
}

/// The protocol-supported reaction identity. Custom emoji remain identified by
/// their Telegram document ID and are never silently converted to ordinary emoji.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum MessageReactionType {
    Emoji(String),
    CustomEmoji(i64),
}

impl MessageReactionType {
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::CoreError;

        match self {
            Self::Emoji(value)
                if !value.trim().is_empty()
                    && value.len() <= crate::command::MAX_REACTION_BYTES =>
            {
                Ok(())
            }
            Self::Emoji(_) => Err(CoreError::InvalidInput(
                "reaction emoji must be non-empty and within the byte limit".into(),
            )),
            Self::CustomEmoji(id) if *id > 0 => Ok(()),
            Self::CustomEmoji(_) => Err(CoreError::InvalidInput(
                "custom emoji document id must be positive".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageReaction {
    pub reaction: MessageReactionType,
    pub count: u32,
    pub chosen_by_me: bool,
}

/// Orders snapshots within a Telegram session and connection epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ReactionRevision {
    pub token: crate::connection_machine::ConnectionToken,
    pub sequence: u64,
}

/// A validated WGS84 point. Construction and deserialization reject non-finite
/// values and coordinates outside Telegram's latitude/longitude ranges.
#[derive(Debug, Clone, Serialize)]
pub struct GeoPoint {
    latitude: f64,
    longitude: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    accuracy_radius_meters: Option<i32>,
}

impl GeoPoint {
    pub fn new(
        latitude: f64,
        longitude: f64,
        accuracy_radius_meters: Option<i32>,
    ) -> crate::error::Result<Self> {
        let point = Self {
            latitude,
            longitude,
            accuracy_radius_meters,
        };
        point.validate()?;
        Ok(point)
    }

    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::CoreError;

        if !self.latitude.is_finite() {
            return Err(CoreError::InvalidInput("latitude must be finite".into()));
        }
        if !self.longitude.is_finite() {
            return Err(CoreError::InvalidInput("longitude must be finite".into()));
        }
        if !(-90.0..=90.0).contains(&self.latitude) {
            return Err(CoreError::InvalidInput(
                "latitude must be in -90..=90".into(),
            ));
        }
        if !(-180.0..=180.0).contains(&self.longitude) {
            return Err(CoreError::InvalidInput(
                "longitude must be in -180..=180".into(),
            ));
        }
        if self
            .accuracy_radius_meters
            .is_some_and(|radius| !(0..=1_500).contains(&radius))
        {
            return Err(CoreError::InvalidInput(
                "accuracy radius must be in 0..=1500 meters".into(),
            ));
        }
        Ok(())
    }

    pub const fn latitude(&self) -> f64 {
        self.latitude
    }

    pub const fn longitude(&self) -> f64 {
        self.longitude
    }

    pub const fn accuracy_radius_meters(&self) -> Option<i32> {
        self.accuracy_radius_meters
    }
}

impl PartialEq for GeoPoint {
    fn eq(&self, other: &Self) -> bool {
        self.latitude == other.latitude
            && self.longitude == other.longitude
            && self.accuracy_radius_meters == other.accuracy_radius_meters
    }
}

impl Eq for GeoPoint {}

impl<'de> Deserialize<'de> for GeoPoint {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            latitude: f64,
            longitude: f64,
            #[serde(default)]
            accuracy_radius_meters: Option<i32>,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.latitude, wire.longitude, wire.accuracy_radius_meters)
            .map_err(de::Error::custom)
    }
}

/// Backend-neutral Telegram venue metadata. Telegram may omit the geo point;
/// title and address are required for a useful venue card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Venue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    point: Option<GeoPoint>,
    title: String,
    address: String,
    provider: String,
    venue_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    venue_type: Option<String>,
}

impl Venue {
    pub fn new(
        point: Option<GeoPoint>,
        title: String,
        address: String,
        provider: String,
        venue_id: String,
        venue_type: Option<String>,
    ) -> crate::error::Result<Self> {
        let venue = Self {
            point,
            title,
            address,
            provider,
            venue_id,
            venue_type: venue_type.filter(|value| !value.is_empty()),
        };
        venue.validate()?;
        Ok(venue)
    }

    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::CoreError;

        if self.title.trim().is_empty() {
            return Err(CoreError::InvalidInput("venue title is empty".into()));
        }
        if self.address.trim().is_empty() {
            return Err(CoreError::InvalidInput("venue address is empty".into()));
        }
        if let Some(point) = &self.point {
            point.validate()?;
        }
        Ok(())
    }

    pub const fn point(&self) -> Option<&GeoPoint> {
        self.point.as_ref()
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn venue_id(&self) -> &str {
        &self.venue_id
    }

    pub fn venue_type(&self) -> Option<&str> {
        self.venue_type.as_deref()
    }
}

impl<'de> Deserialize<'de> for Venue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            #[serde(default)]
            point: Option<GeoPoint>,
            title: String,
            address: String,
            provider: String,
            venue_id: String,
            #[serde(default)]
            venue_type: Option<String>,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.point,
            wire.title,
            wire.address,
            wire.provider,
            wire.venue_id,
            wire.venue_type,
        )
        .map_err(de::Error::custom)
    }
}

/// Location media carried by a Telegram message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageLocation {
    Location { point: GeoPoint },
    Venue { venue: Venue },
}

impl MessageLocation {
    pub fn validate(&self) -> crate::error::Result<()> {
        match self {
            Self::Location { point } => point.validate(),
            Self::Venue { venue } => venue.validate(),
        }
    }

    pub fn point(&self) -> Option<&GeoPoint> {
        match self {
            Self::Location { point } => Some(point),
            Self::Venue { venue } => venue.point(),
        }
    }
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
    Queued,
    Downloading {
        bytes_transferred: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total_bytes: Option<u64>,
    },
    Downloaded {
        local_path: PathBuf,
    },
    Failed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Cancelled,
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
    /// Telegram's packed 5-bit voice waveform, when included in the audio attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_waveform: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<AttachmentThumbnail>,
    /// Identifies the latest download attempt. Older asynchronous transfer
    /// events are ignored after a cancellation or retry.
    #[serde(default)]
    pub download_generation: u64,
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
    Offline {
        last_seen_unix: i64,
    },
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
    /// Local private-cache path used by Android to render the avatar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<PathBuf>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_bot: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_scam: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_premium: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_send_messages: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_invite_users: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_change_info: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_post_messages: Option<bool>,
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
            is_bot: None,
            is_verified: None,
            is_scam: None,
            is_premium: None,
            can_send_messages: None,
            can_invite_users: None,
            can_change_info: None,
            can_post_messages: None,
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
