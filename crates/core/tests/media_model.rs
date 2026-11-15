use serde_json::json;
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
        filename: Some("media.webp".into()),
        thumbnail: Some(AttachmentThumbnail {
            file_reference: "thumbnail-reference".into(),
            size_bytes: Some(8),
            mime_type: Some("image/jpeg".into()),
            width: Some(32),
            height: Some(24),
        }),
        download_state: AttachmentDownloadState::NotDownloaded,
    }
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

    assert!(decoded
        .as_ref()
        .is_some_and(|state| state
            .messages
            .get(&3)
            .and_then(|messages| messages.first())
            .is_some_and(|message| message.attachments.is_empty())));
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
        attachments: Vec::new(),
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
