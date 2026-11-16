use std::fs;

use tg_mobile_core::{
    decode_command, CoreCommand, CoreEvent, CoreState, GeoPoint, JsonFileStateStore,
    MessageLocation, MessageSummary, StateStore, Venue,
};

fn point() -> GeoPoint {
    GeoPoint::new(52.229_675_6, 21.012_228_7, Some(12)).unwrap()
}

fn location_message() -> MessageSummary {
    MessageSummary {
        id: 101,
        chat_id: 7,
        sender_id: 42,
        text: String::new(),
        timestamp_unix: 1_700_000_000,
        outgoing: false,
        service: None,
        location: Some(MessageLocation::Location { point: point() }),
        attachments: Vec::new(),
        forwarded_from: None,
        edited: false,
        edit_timestamp_unix: None,
        service_message: false,
        media_caption: None,
        reply_to: None,
        reactions: Vec::new(),
        reaction_revision: None,
    }
}

fn venue_message() -> MessageSummary {
    let venue = Venue::new(
        Some(point()),
        "Central Library".into(),
        "1 Example Street".into(),
        "foursquare".into(),
        "venue-1".into(),
        Some("library".into()),
    )
    .unwrap();
    MessageSummary {
        id: 102,
        location: Some(MessageLocation::Venue { venue }),
        ..location_message()
    }
}

#[test]
fn latitude_boundaries_are_accepted_and_outside_rejected() {
    assert!(GeoPoint::new(-90.0, 0.0, None).is_ok());
    assert!(GeoPoint::new(90.0, 0.0, None).is_ok());
    assert!(GeoPoint::new(-90.000_001, 0.0, None).is_err());
    assert!(GeoPoint::new(90.000_001, 0.0, None).is_err());
}

#[test]
fn longitude_boundaries_are_accepted_and_outside_rejected() {
    assert!(GeoPoint::new(0.0, -180.0, None).is_ok());
    assert!(GeoPoint::new(0.0, 180.0, None).is_ok());
    assert!(GeoPoint::new(0.0, -180.000_001, None).is_err());
    assert!(GeoPoint::new(0.0, 180.000_001, None).is_err());
}

#[test]
fn non_finite_coordinates_and_negative_accuracy_are_rejected() {
    assert!(GeoPoint::new(f64::NAN, 0.0, None).is_err());
    assert!(GeoPoint::new(f64::INFINITY, 0.0, None).is_err());
    assert!(GeoPoint::new(0.0, f64::NEG_INFINITY, None).is_err());
    assert!(GeoPoint::new(0.0, 0.0, Some(-1)).is_err());
    assert!(GeoPoint::new(0.0, 0.0, Some(0)).is_ok());
    assert!(GeoPoint::new(0.0, 0.0, Some(1_500)).is_ok());
    assert!(GeoPoint::new(0.0, 0.0, Some(1_501)).is_err());
}

#[test]
fn venue_requires_human_readable_title_and_address() {
    assert!(Venue::new(
        None,
        " ".into(),
        "address".into(),
        "".into(),
        "".into(),
        None
    )
    .is_err());
    assert!(Venue::new(None, "title".into(), " ".into(), "".into(), "".into(), None).is_err());
}

#[test]
fn location_and_venue_json_round_trip() {
    for original in [location_message(), venue_message()] {
        let json = serde_json::to_string(&original).unwrap();
        let decoded: MessageSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);
    }
}

#[test]
fn older_message_json_without_location_remains_compatible() {
    let json = r#"{
        "id":1,"chat_id":7,"sender_id":42,"text":"old",
        "timestamp_unix":1,"outgoing":false
    }"#;
    let decoded: MessageSummary = serde_json::from_str(json).unwrap();
    assert!(decoded.location.is_none());
}

#[test]
fn send_location_command_round_trips_through_canonical_wire() {
    let command = CoreCommand::SendLocation {
        chat_id: 7,
        location: point(),
    };
    let encoded = serde_json::to_string(&command).unwrap();
    let decoded = decode_command(&encoded).unwrap();
    assert_eq!(decoded, command);
    decoded.validate().unwrap();
}

#[test]
fn history_reducer_preserves_location_and_duplicate_update_is_idempotent() {
    let message = location_message();
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessagesLoaded {
        chat_id: message.chat_id,
        messages: vec![message.clone()],
    });
    assert_eq!(state.messages_for_chat(message.chat_id), &[message.clone()]);

    state.reduce(&CoreEvent::MessageAdded(message.clone()));
    state.reduce(&CoreEvent::MessageAdded(message.clone()));
    assert_eq!(state.messages_for_chat(message.chat_id), &[message]);
}

#[tokio::test]
async fn location_survives_json_file_persistence() {
    let path = std::env::temp_dir().join(format!(
        "tg-mobile-location-persistence-{}.json",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    let store = JsonFileStateStore::new(&path);

    let message = venue_message();
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessagesLoaded {
        chat_id: message.chat_id,
        messages: vec![message.clone()],
    });
    store.save(&state).await.unwrap();

    let restored = store.load().await.unwrap().unwrap();
    assert_eq!(restored.messages_for_chat(message.chat_id), &[message]);

    let _ = fs::remove_file(path);
}
