use tg_mobile_core::{
    decode_command, CoreCommand, CoreEvent, CoreState, MemoryStateStore, PeerKind, PeerPresence,
    PeerProfile, StateStore,
};

#[test]
fn load_profile_command_round_trips_and_validates_peer_id() {
    let command = CoreCommand::LoadProfile { peer_id: -42 };
    let encoded = serde_json::to_string(&command).expect("command serializes");
    assert_eq!(decode_command(&encoded).expect("command decodes"), command);
    assert!(command.validate().is_ok());
    assert!(CoreCommand::LoadProfile { peer_id: 0 }.validate().is_err());
}

#[test]
fn profile_event_preserves_supplied_fields_and_omits_absent_phone() {
    let profile = PeerProfile {
        peer_id: 42,
        kind: PeerKind::Bot,
        display_name: Some("Example".into()),
        first_name: Some("Example".into()),
        last_name: None,
        username: Some("example_bot".into()),
        phone: None,
        is_bot: Some(true),
        is_verified: Some(false),
        is_scam: None,
        is_fake: None,
        member_count: None,
        bio: Some("Telegram supplied bio".into()),
        avatar: None,
        presence: PeerPresence::Offline { last_seen_unix: 1_700_000_000 },
    };
    let event = CoreEvent::ProfileUpdated(profile);
    let encoded = tg_mobile_core::encode_event(&event).expect("event serializes");
    let decoded: CoreEvent = serde_json::from_str(&encoded).expect("event decodes");
    assert_eq!(decoded, event);
    assert!(!encoded.contains("\"phone\""));
    assert!(!encoded.contains("\"is_scam\""));
    assert_eq!(event.kind(), "profile_updated");
}

#[test]
fn profile_reducer_keeps_stable_id_and_old_snapshots_migrate() {
    let profile = PeerProfile::unknown(-42);
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ProfileUpdated(profile.clone()));
    assert_eq!(state.profile(-42), Some(&profile));

    let encoded = serde_json::to_string(&state).expect("state serializes");
    let restored: CoreState = serde_json::from_str(&encoded).expect("state restores");
    assert_eq!(restored.profile(-42), Some(&profile));

    let old_state = r#"{ "running": true, "account": { "authorization": "Closed", "chats": [] }, "messages": {}, "last_error": null }"#;
    let migrated: CoreState = serde_json::from_str(old_state).expect("old snapshot migrates");
    assert!(migrated.profiles.is_empty());
}

#[tokio::test]
async fn peer_profiles_are_persisted_by_the_existing_store() {
    let store = MemoryStateStore::default();
    let mut state = CoreState::default();
    let profile = PeerProfile::unknown(73);
    state.reduce(&CoreEvent::ProfileUpdated(profile.clone()));
    store.save(&state).await.expect("state saves");
    let restored = store.load().await.expect("state loads").expect("state exists");
    assert_eq!(restored.profile(73), Some(&profile));
}
