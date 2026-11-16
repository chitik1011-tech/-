use tg_mobile_core::{
    decode_command, encode_event, CoreCommand, CoreEvent, CoreState, PeerKind, PeerPresence,
    PeerProfile, MAX_CACHED_PROFILES,
};

fn profile() -> PeerProfile {
    PeerProfile {
        peer_id: -73,
        kind: PeerKind::Bot,
        display_name: Some("Example bot".into()),
        first_name: Some("Example".into()),
        last_name: Some("bot".into()),
        username: Some("example_bot".into()),
        is_bot: Some(true),
        is_verified: Some(true),
        is_scam: None,
        is_fake: None,
        is_premium: Some(false),
        member_count: None,
        bio: Some("Telegram supplied bio".into()),
        avatar: None,
        presence: PeerPresence::Online,
        can_send_messages: None,
        can_invite_users: None,
        can_change_info: None,
        can_post_messages: None,
    }
}

#[test]
fn load_profile_wire_command_validates_stable_peer_id() {
    let command = CoreCommand::LoadProfile { peer_id: -73 };
    let json = serde_json::to_string(&command).unwrap();
    assert_eq!(decode_command(&json).unwrap(), command);
    assert!(command.validate().is_ok());
    assert!(CoreCommand::LoadProfile { peer_id: 0 }.validate().is_err());
}

#[test]
fn profile_update_round_trips_without_phone_number() {
    let event = CoreEvent::ProfileUpdated(profile());
    let json = encode_event(&event).unwrap();
    let decoded: CoreEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, event);
    assert!(!json.contains("phone"));
    assert_eq!(event.kind(), "profile_updated");
}

#[test]
fn profile_cache_is_bounded_and_evicts_oldest_entries() {
    let profile = profile();
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ProfileUpdated(profile.clone()));
    assert_eq!(state.profile(profile.peer_id), Some(&profile));
    for peer_id in 1..=(MAX_CACHED_PROFILES as i64) {
        let mut next = profile.clone();
        next.peer_id = peer_id;
        state.reduce(&CoreEvent::ProfileUpdated(next));
    }
    assert_eq!(state.profiles.len(), MAX_CACHED_PROFILES);
    assert!(state.profile(-73).is_none());
    assert!(state.profile(1).is_some());
}

#[test]
fn persistence_snapshot_omits_transient_personal_profiles() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ProfileUpdated(profile()));
    let snapshot = state.persistence_snapshot();
    assert!(snapshot.profiles.is_empty());
    assert!(snapshot.profile_order.is_empty());
    assert!(state.profile(-73).is_some());
}

#[test]
fn old_snapshots_load_with_empty_profile_map() {
    let old = r#"{"running":true,"account":{"authorization":"Closed","chats":[]},"messages":{},"last_error":null}"#;
    let state: CoreState = serde_json::from_str(old).unwrap();
    assert!(state.profiles.is_empty());
}
