use serde_json::json;
use tg_mobile_core::{ForwardedFrom, MessageSummary, ReplyMetadata};

#[test]
fn legacy_message_defaults_metadata_without_changing_old_wire_shape() {
    let legacy = json!({
        "id": 7,
        "chat_id": 3,
        "sender_id": 9,
        "text": "old message",
        "timestamp_unix": 100,
        "outgoing": false
    });

    let message: MessageSummary =
        serde_json::from_value(legacy.clone()).expect("legacy message should deserialize");

    assert!(message.attachments.is_empty());
    assert_eq!(message.forwarded_from, None);
    assert!(!message.edited);
    assert_eq!(message.edit_timestamp_unix, None);
    assert!(!message.service_message);
    assert_eq!(message.media_caption, None);
    assert_eq!(message.reply_to, None);

    assert_eq!(
        serde_json::to_value(message).expect("message should serialize"),
        legacy
    );
}

#[test]
fn message_metadata_round_trips_with_media_compatible_defaults() {
    let message = MessageSummary {
        id: 11,
        chat_id: 3,
        sender_id: 9,
        text: "forwarded reply".into(),
        timestamp_unix: 200,
        outgoing: false,
        attachments: Vec::new(),
        forwarded_from: Some(ForwardedFrom {
            sender_id: Some(12),
            chat_id: Some(4),
            channel_id: Some(5),
            timestamp_unix: Some(150),
            sender_display_name: Some("Original author".into()),
        }),
        edited: true,
        edit_timestamp_unix: Some(210),
        service_message: true,
        service: None,
        media_caption: Some("photo caption".into()),
        reply_to: Some(ReplyMetadata {
            message_id: 8,
            chat_id: Some(3),
            sender_id: Some(10),
            sender_display_name: Some("Reply target".into()),
        }),

        reactions: Vec::new(),
        reaction_revision: None,
    };

    let encoded = serde_json::to_string(&message).expect("message should serialize");
    let decoded: MessageSummary =
        serde_json::from_str(&encoded).expect("message should deserialize");

    assert_eq!(decoded, message);
}
