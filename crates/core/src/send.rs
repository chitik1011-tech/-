use serde::{Deserialize, Serialize};
use std::path::Path;

/// Stable client-generated identifier for one logical outgoing message.
/// The same identifier may be retried with the same payload. For messages,
/// Telegram uses it as the MTProto `random_id` idempotency key.
pub type ClientMessageId = i64;

pub const MAX_OUTGOING_MEDIA_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutgoingMediaKind {
    Photo,
    Video,
    Document,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingMediaAttachment {
    pub kind: OutgoingMediaKind,
    /// App-private absolute path copied from Photo Picker or SAF.
    pub path: String,
    pub file_name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_height: Option<u32>,
}

impl OutgoingMediaAttachment {
    pub fn validate(&self) -> Result<(), String> {
        if self.path.len() > 4096
            || !Path::new(&self.path).is_absolute()
            || self.path.contains('\0')
        {
            return Err("media path must be an absolute local path".into());
        }
        if self.file_name.trim().is_empty()
            || self.file_name.len() > 255
            || self.file_name.chars().any(char::is_control)
        {
            return Err("media file name is invalid".into());
        }
        if self.mime_type.trim().is_empty()
            || self.mime_type.len() > 127
            || self.mime_type.chars().any(char::is_control)
        {
            return Err("media MIME type is invalid".into());
        }
        if self.size_bytes == 0 || self.size_bytes > MAX_OUTGOING_MEDIA_BYTES {
            return Err(format!(
                "media size must be between 1 byte and {MAX_OUTGOING_MEDIA_BYTES} bytes"
            ));
        }
        if self.kind != OutgoingMediaKind::Video
            && (self.video_duration_ms.is_some()
                || self.video_width.is_some()
                || self.video_height.is_some())
        {
            return Err("video metadata can only be set for video media".into());
        }
        if self
            .video_duration_ms
            .is_some_and(|duration| duration > 48 * 60 * 60 * 1000)
        {
            return Err("video duration exceeds the 48 hour upload limit".into());
        }
        match (self.video_width, self.video_height) {
            (None, None) => {}
            (Some(width), Some(height))
                if width > 0 && height > 0 && width <= 32768 && height <= 32768 => {}
            _ => {
                return Err("video dimensions must be provided together and be in 1..=32768".into())
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingSend {
    pub client_id: ClientMessageId,
    pub chat_id: i64,
    pub text: String,
    /// Absent on older persisted text outbox entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<OutgoingMediaAttachment>,
    /// TransferManager generation used to reject late upload callbacks.
    #[serde(default)]
    pub transfer_generation: u64,
    #[serde(default)]
    pub session_generation: u64,
    #[serde(default)]
    pub session_epoch: u64,
    #[serde(default)]
    pub bytes_transferred: u64,
    pub state: OutgoingSendState,
}

impl OutgoingSend {
    pub fn sending(client_id: ClientMessageId, chat_id: i64, text: impl Into<String>) -> Self {
        Self::text_send(client_id, chat_id, text, OutgoingSendState::Sending)
    }

    pub fn sent(
        client_id: ClientMessageId,
        chat_id: i64,
        text: impl Into<String>,
        message_id: i64,
    ) -> Self {
        Self::text_send(
            client_id,
            chat_id,
            text,
            OutgoingSendState::Sent { message_id },
        )
    }

    pub fn failed(
        client_id: ClientMessageId,
        chat_id: i64,
        text: impl Into<String>,
        reason: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self::text_send(
            client_id,
            chat_id,
            text,
            OutgoingSendState::Failed {
                reason: reason.into(),
                retryable,
            },
        )
    }

    pub fn awaiting_confirmation(
        client_id: ClientMessageId,
        chat_id: i64,
        text: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::text_send(
            client_id,
            chat_id,
            text,
            OutgoingSendState::AwaitingConfirmation {
                reason: reason.into(),
            },
        )
    }

    pub fn flood_wait(
        client_id: ClientMessageId,
        chat_id: i64,
        text: impl Into<String>,
        seconds: u32,
    ) -> Self {
        Self::text_send(
            client_id,
            chat_id,
            text,
            OutgoingSendState::FloodWait { seconds },
        )
    }

    pub fn rejected(
        client_id: ClientMessageId,
        chat_id: i64,
        text: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::text_send(
            client_id,
            chat_id,
            text,
            OutgoingSendState::Rejected {
                reason: reason.into(),
            },
        )
    }

    pub fn uploading_media(
        client_id: ClientMessageId,
        chat_id: i64,
        media: OutgoingMediaAttachment,
        caption: impl Into<String>,
        bytes_transferred: u64,
        transfer_generation: u64,
    ) -> Self {
        Self {
            client_id,
            chat_id,
            text: caption.into(),
            media: Some(media.clone()),
            transfer_generation,
            session_generation: 0,
            session_epoch: 0,
            bytes_transferred,
            state: OutgoingSendState::Uploading {
                bytes_transferred,
                total_bytes: media.size_bytes,
            },
        }
    }

    pub fn sending_media(
        client_id: ClientMessageId,
        chat_id: i64,
        media: OutgoingMediaAttachment,
        caption: impl Into<String>,
        transfer_generation: u64,
    ) -> Self {
        Self {
            client_id,
            chat_id,
            text: caption.into(),
            media: Some(media),
            transfer_generation,
            session_generation: 0,
            session_epoch: 0,
            bytes_transferred: 0,
            state: OutgoingSendState::Sending,
        }
    }

    pub fn sent_media(
        client_id: ClientMessageId,
        chat_id: i64,
        media: OutgoingMediaAttachment,
        caption: impl Into<String>,
        message_id: i64,
        transfer_generation: u64,
    ) -> Self {
        Self {
            client_id,
            chat_id,
            text: caption.into(),
            media: Some(media),
            transfer_generation,
            session_generation: 0,
            session_epoch: 0,
            bytes_transferred: 0,
            state: OutgoingSendState::Sent { message_id },
        }
    }

    pub fn failed_media(
        client_id: ClientMessageId,
        chat_id: i64,
        media: OutgoingMediaAttachment,
        caption: impl Into<String>,
        reason: impl Into<String>,
        retryable: bool,
        transfer_generation: u64,
    ) -> Self {
        Self {
            client_id,
            chat_id,
            text: caption.into(),
            media: Some(media),
            transfer_generation,
            session_generation: 0,
            session_epoch: 0,
            bytes_transferred: 0,
            state: OutgoingSendState::Failed {
                reason: reason.into(),
                retryable,
            },
        }
    }

    pub fn awaiting_confirmation_media(
        client_id: ClientMessageId,
        chat_id: i64,
        media: OutgoingMediaAttachment,
        caption: impl Into<String>,
        reason: impl Into<String>,
        transfer_generation: u64,
    ) -> Self {
        Self {
            client_id,
            chat_id,
            text: caption.into(),
            media: Some(media),
            transfer_generation,
            session_generation: 0,
            session_epoch: 0,
            bytes_transferred: 0,
            state: OutgoingSendState::AwaitingConfirmation {
                reason: reason.into(),
            },
        }
    }

    fn text_send(
        client_id: ClientMessageId,
        chat_id: i64,
        text: impl Into<String>,
        state: OutgoingSendState,
    ) -> Self {
        Self {
            client_id,
            chat_id,
            text: text.into(),
            media: None,
            transfer_generation: 0,
            session_generation: 0,
            session_epoch: 0,
            bytes_transferred: 0,
            state,
        }
    }

    pub fn with_session_token(mut self, generation: u64, epoch: u64) -> Self {
        self.session_generation = generation;
        self.session_epoch = epoch;
        self
    }

    pub fn same_request(&self, chat_id: i64, text: &str) -> bool {
        self.chat_id == chat_id && self.text == text && self.media.is_none()
    }

    pub fn same_media_request(
        &self,
        chat_id: i64,
        media: &OutgoingMediaAttachment,
        caption: &str,
    ) -> bool {
        self.chat_id == chat_id && self.text == caption && self.media.as_ref() == Some(media)
    }

    pub fn same_payload(&self, other: &Self) -> bool {
        self.chat_id == other.chat_id && self.text == other.text && self.media == other.media
    }

    pub fn with_upload_progress(&self, bytes_transferred: u64, transfer_generation: u64) -> Self {
        let mut update = self.clone();
        update.transfer_generation = transfer_generation;
        update.bytes_transferred = bytes_transferred;
        if let Some(media) = &self.media {
            update.state = OutgoingSendState::Uploading {
                bytes_transferred,
                total_bytes: media.size_bytes,
            };
        }
        update
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OutgoingSendState {
    Sending,
    Uploading {
        bytes_transferred: u64,
        total_bytes: u64,
    },
    AwaitingConfirmation {
        reason: String,
    },
    FloodWait {
        seconds: u32,
    },
    Sent {
        message_id: i64,
    },
    Failed {
        reason: String,
        retryable: bool,
    },
    Rejected {
        reason: String,
    },
}

impl OutgoingSendState {
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Sent { .. } | Self::Rejected { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_media() -> OutgoingMediaAttachment {
        OutgoingMediaAttachment {
            kind: OutgoingMediaKind::Photo,
            path: "/data/user/0/example/files/outgoing-media/photo.jpg".into(),
            file_name: "photo.jpg".into(),
            mime_type: "image/jpeg".into(),
            size_bytes: 512,
            video_duration_ms: None,
            video_width: None,
            video_height: None,
        }
    }

    #[test]
    fn media_payload_requires_a_safe_absolute_path_and_bounded_metadata() {
        assert!(valid_media().validate().is_ok());
        let mut media = valid_media();
        media.path = "content://picker/photo.jpg".into();
        assert!(media.validate().is_err());
        media = valid_media();
        media.size_bytes = MAX_OUTGOING_MEDIA_BYTES + 1;
        assert!(media.validate().is_err());
    }

    #[test]
    fn video_metadata_is_bounded_and_only_valid_for_video() {
        let mut video = valid_media();
        video.kind = OutgoingMediaKind::Video;
        video.video_duration_ms = Some(90_000);
        video.video_width = Some(1920);
        video.video_height = Some(1080);
        assert!(video.validate().is_ok());

        video.video_width = None;
        assert!(video.validate().is_err());
        video.video_width = Some(1920);
        video.video_duration_ms = Some(48 * 60 * 60 * 1000 + 1);
        assert!(video.validate().is_err());

        let mut photo = valid_media();
        photo.video_width = Some(100);
        photo.video_height = Some(100);
        assert!(photo.validate().is_err());
    }

    #[test]
    fn media_retries_match_complete_payload() {
        let media = valid_media();
        let send = OutgoingSend::uploading_media(7, 42, media.clone(), "caption", 10, 2);
        assert!(send.same_media_request(42, &media, "caption"));
        assert!(!send.same_media_request(42, &media, "different"));
        assert!(!send.same_media_request(43, &media, "caption"));
    }

    #[test]
    fn legacy_text_outbox_entry_defaults_new_media_fields() {
        let value = serde_json::json!({
            "client_id": 7,
            "chat_id": 42,
            "text": "hello",
            "state": {"status": "sending"}
        });
        let send: OutgoingSend = serde_json::from_value(value).unwrap();
        assert!(send.media.is_none());
        assert_eq!(send.transfer_generation, 0);
        assert_eq!(send.bytes_transferred, 0);
    }
}
