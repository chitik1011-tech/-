use tg_mobile_core::{
    decode_command, CoreCommand, CoreEvent, CoreState, JsonFileStateStore, MemoryStateStore,
    PeerKind, PeerPresence, PeerProfile, StateStore,
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
        presence: PeerPresence::Offline {
            last_seen_unix: 1_700_000_000,
        },
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
async fn file_persistence_drops_phone_but_keeps_other_profile_fields() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "tg-mobile-peer-profiles-{}-{unique}.json",
        std::process::id()
    ));
    let store = JsonFileStateStore::new(&path);
    let mut state = CoreState::default();
    let mut profile = PeerProfile::unknown(73);
    profile.username = Some("member".into());
    profile.phone = Some("+15551234567".into());
    profile.bio = Some("Telegram supplied bio".into());
    state.reduce(&CoreEvent::ProfileUpdated(profile.clone()));

    let event_json = tg_mobile_core::encode_event(&CoreEvent::ProfileUpdated(profile))
        .expect("profile event serializes");
    assert!(event_json.contains("\\"phone\\":\\"+15551234567\\""));

    store.save(&state).await.expect("state saves");
    let persisted_json = std::fs::read_to_string(store.path()).expect("persisted JSON reads");
    assert!(!persisted_json.contains("\\"phone\\""));
    assert!(persisted_json.contains("\\"username\\":\\"member\\""));
    assert!(persisted_json.contains("\\"bio\\":\\"Telegram supplied bio\\""));

    let restored = store
        .load()
        .await
        .expect("state loads")
        .expect("state exists");
    let restored_profile = restored.profile(73).expect("profile exists");
    assert_eq!(restored_profile.phone, None);
    assert_eq!(restored_profile.username.as_deref(), Some("member"));
    assert_eq!(restored_profile.bio.as_deref(), Some("Telegram supplied bio"));
    let _ = std::fs::remove_file(store.path());
}

#[tokio::test]
async fn memory_store_drops_phone_fields_too() {
    let store = MemoryStateStore::default();
    let mut state = CoreState::default();
    let mut profile = PeerProfile::unknown(74);
    profile.phone = Some("+15559876543".into());
    profile.username = Some("cached".into());
    state.reduce(&CoreEvent::ProfileUpdated(profile));

    store.save(&state).await.expect("state saves");
    let restored = store.load().await.expect("state loads").expect("state exists");
    let restored_profile = restored.profile(74).expect("profile exists");
    assert_eq!(restored_profile.phone, None);
    assert_eq!(restored_profile.username.as_deref(), Some("cached"));
}
