use serde_json::json;
use tg_mobile_core::model::MediaDimensions;
use tg_mobile_core::{
    AttachmentDownloadState, AttachmentKind, AttachmentThumbnail, MediaAttachment, MessageSummary,
};

fn media(kind: AttachmentKind) -> MediaAttachment {
    MediaAttachment {
        kind,
        file_reference: "telegram-file-reference".into(),
        size_bytes: Some(42),
        mime_type: Some("image/webp".into()),
        width: Some(120),
        height: Some(80),
        duration_ms: Some(1_250),
        voice_waveform: None,
        filename: Some("media.webp".into()),
        thumbnail: Some(AttachmentThumbnail {
            file_reference: "thumbnail-reference".into(),
            size_bytes: Some(8),
            mime_type: Some("image/jpeg".into()),
            width: Some(32),
            height: Some(24),
        }),
        download_generation: 0,
        download_state: AttachmentDownloadState::NotDownloaded,
    }
}

#[test]
fn optional_voice_waveform_round_trips_without_breaking_legacy_media() {
    let mut attachment = media(AttachmentKind::Voice);
    attachment.voice_waveform = Some(vec![0xff, 0x03]);
    let encoded = serde_json::to_value(&attachment).expect("serialize voice media");
    assert_eq!(encoded["voice_waveform"], json!([255, 3]));
    assert_eq!(
        serde_json::from_value::<MediaAttachment>(encoded).expect("deserialize voice media"),
        attachment,
    );

    let mut legacy = serde_json::to_value(media(AttachmentKind::Voice)).expect("serialize media");
    legacy
        .as_object_mut()
        .expect("media object")
        .remove("voice_waveform");
    let decoded = serde_json::from_value::<MediaAttachment>(legacy).expect("legacy media");
    assert_eq!(decoded.voice_waveform, None);
}

#[test]
fn legacy_message_without_attachments_migrates_to_empty_list() {
    let old_message = json!({
        "id": 7,
        "chat_id": 3,
        "sender_id": 2,
        "text": "",
        "timestamp_unix": 1_700_000_000,
        "outgoing": false
    });
    let decoded = serde_json::from_value::<MessageSummary>(old_message.clone()).ok();

    assert!(decoded
        .as_ref()
        .is_some_and(|message| message.attachments.is_empty()));

    let encoded = decoded.and_then(|message| serde_json::to_value(message).ok());
    assert_eq!(encoded, Some(old_message));
}

#[test]
fn legacy_persisted_core_state_migrates_message_media_field() {
    let old_state = json!({
        "running": true,
        "account": {
            "authorization": "Closed",
            "chats": [{
                "id": 3,
                "title": "Example",
                "unread_count": 0,
                "last_message": null
            }]
        },
        "messages": {
            "3": [{
                "id": 7,
                "chat_id": 3,
                "sender_id": 2,
                "text": "",
                "timestamp_unix": 1_700_000_000,
                "outgoing": false
            }]
        },
        "last_error": null
    });
    let decoded = serde_json::from_value::<tg_mobile_core::CoreState>(old_state).ok();

    let message_has_empty_attachments = decoded
        .as_ref()
        .and_then(|state| state.messages.get(&3))
        .and_then(|messages| messages.first())
        .map(|message| message.attachments.is_empty());

    assert_eq!(message_has_empty_attachments, Some(true));
}

#[test]
fn empty_attachment_list_keeps_old_message_wire_shape() {
    let message = MessageSummary {
        id: 7,
        chat_id: 3,
        sender_id: 2,
        text: String::new(),
        timestamp_unix: 1_700_000_000,
        outgoing: false,
        location: None,
        attachments: Vec::new(),
        forwarded_from: None,
        edited: false,
        edit_timestamp_unix: None,
        service_message: false,
        service: None,
        media_caption: None,
        reply_to: None,

        reactions: Vec::new(),
        reaction_revision: None,
    };
    let encoded = serde_json::to_value(message).ok();

    assert!(encoded
        .as_ref()
        .is_some_and(|value| value.get("attachments").is_none()));
}

#[test]
fn all_telegram_media_kinds_round_trip_with_metadata() {
    let kinds = [
        AttachmentKind::Photo,
        AttachmentKind::Video,
        AttachmentKind::Document,
        AttachmentKind::Audio,
        AttachmentKind::Voice,
        AttachmentKind::Animation,
        AttachmentKind::Sticker,
        AttachmentKind::VideoNote,
    ];

    for kind in kinds {
        let attachment = media(kind);
        let encoded = serde_json::to_value(&attachment).ok();
        let decoded = encoded
            .clone()
            .and_then(|value| serde_json::from_value::<MediaAttachment>(value).ok());

        assert_eq!(decoded, Some(attachment));
    }
}

#[test]
fn undownloaded_state_does_not_expose_a_local_path() {
    let attachment = media(AttachmentKind::Document);
    let state = serde_json::to_value(&attachment)
        .ok()
        .and_then(|value| value.get("download_state").cloned());

    assert!(state
        .as_ref()
        .is_some_and(|value| value.get("local_path").is_none()));
}

#[test]
fn telegram_dimensions_normalize_positive_values_and_reject_invalid_boundaries() {
    let dimensions = MediaDimensions::from_telegram(1920, 1080);
    assert_eq!(dimensions.map(MediaDimensions::width), Some(1920));
    assert_eq!(dimensions.map(MediaDimensions::height), Some(1080));
    assert_eq!(
        MediaDimensions::from_telegram(i32::MAX, i32::MAX).map(MediaDimensions::width),
        Some(i32::MAX as u32)
    );

    assert_eq!(MediaDimensions::from_telegram(0, 1080), None);
    assert_eq!(MediaDimensions::from_telegram(1920, 0), None);
    assert_eq!(MediaDimensions::from_telegram(-1, 1080), None);
    assert_eq!(MediaDimensions::from_telegram(1920, i32::MIN), None);
    assert_eq!(MediaDimensions::new(i32::MAX as u32 + 1, 1080), None);
    assert_eq!(MediaDimensions::new(u32::MAX, 1080), None);
}

#[test]
fn attachment_dimensions_require_a_valid_complete_pair() {
    let mut attachment = media(AttachmentKind::Video);
    assert_eq!(
        attachment.dimensions(),
        MediaDimensions::from_telegram(120, 80)
    );

    attachment.width = None;
    assert_eq!(attachment.dimensions(), None);
    attachment.width = Some(0);
    assert_eq!(attachment.dimensions(), None);
    attachment.width = Some(u32::MAX);
    assert_eq!(attachment.dimensions(), None);

    let mut thumbnail = attachment.thumbnail.clone().expect("test thumbnail exists");
    assert_eq!(
        thumbnail.dimensions(),
        MediaDimensions::from_telegram(32, 24)
    );
    thumbnail.height = None;
    assert_eq!(thumbnail.dimensions(), None);
    thumbnail.height = Some(0);
    assert_eq!(thumbnail.dimensions(), None);
    thumbnail.width = Some(u32::MAX);
    thumbnail.height = Some(24);
    assert_eq!(thumbnail.dimensions(), None);
}

#[test]
fn invalid_deserialized_attachment_dimensions_do_not_bypass_validation() {
    for width in [0, i32::MAX as u32 + 1, u32::MAX] {
        let mut serialized =
            serde_json::to_value(media(AttachmentKind::Photo)).expect("test media serializes");
        serialized["width"] = json!(width);

        let attachment = serde_json::from_value::<MediaAttachment>(serialized)
            .expect("raw media metadata remains deserializable");
        assert_eq!(attachment.dimensions(), None);
    }

    let mut serialized =
        serde_json::to_value(media(AttachmentKind::Photo)).expect("test media serializes");
    serialized["height"] = json!(null);
    let attachment = serde_json::from_value::<MediaAttachment>(serialized)
        .expect("incomplete media metadata remains deserializable");
    assert_eq!(attachment.dimensions(), None);

    let mut serialized =
        serde_json::to_value(media(AttachmentKind::Photo)).expect("test media serializes");
    serialized["width"] = json!(-1);
    assert!(serde_json::from_value::<MediaAttachment>(serialized).is_err());
}

#[test]
fn invalid_deserialized_thumbnail_dimensions_do_not_bypass_validation() {
    for width in [0, i32::MAX as u32 + 1, u32::MAX] {
        let mut serialized =
            serde_json::to_value(media(AttachmentKind::Photo)).expect("test media serializes");
        serialized["thumbnail"]["width"] = json!(width);

        let attachment = serde_json::from_value::<MediaAttachment>(serialized)
            .expect("raw thumbnail metadata remains deserializable");
        assert_eq!(
            attachment
                .thumbnail
                .as_ref()
                .and_then(AttachmentThumbnail::dimensions),
            None
        );
    }

    let mut serialized =
        serde_json::to_value(media(AttachmentKind::Photo)).expect("test media serializes");
    serialized["thumbnail"]["height"] = json!(null);
    let attachment = serde_json::from_value::<MediaAttachment>(serialized)
        .expect("incomplete thumbnail metadata remains deserializable");
    assert_eq!(
        attachment
            .thumbnail
            .as_ref()
            .and_then(AttachmentThumbnail::dimensions),
        None
    );

    let mut serialized =
        serde_json::to_value(media(AttachmentKind::Photo)).expect("test media serializes");
    serialized["thumbnail"]["width"] = json!(-1);
    assert!(serde_json::from_value::<MediaAttachment>(serialized).is_err());
}
