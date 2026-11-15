use serde_json::json;
use tg_mobile_core::{
    CapabilityValue, ChatAction, ChatCapabilities, ChatKind, CoreEvent, CoreState,
};

fn capabilities(chat_id: i64, chat_kind: ChatKind) -> ChatCapabilities {
    ChatCapabilities {
        chat_id,
        chat_kind,
        ..ChatCapabilities::default()
    }
}

#[test]
fn unknown_capabilities_never_grant_actions_or_invent_slow_mode() {
    let snapshot = capabilities(10, ChatKind::Group);

    assert_eq!(
        snapshot.permission(ChatAction::SendMessages),
        CapabilityValue::Unknown
    );
    assert!(!snapshot.allows(ChatAction::SendMessages));
    assert_eq!(snapshot.slow_mode_remaining_seconds, CapabilityValue::Unknown);
}

#[test]
fn group_and_channel_snapshots_validate_and_preserve_explicit_rights() {
    for chat_kind in [ChatKind::Group, ChatKind::Supergroup, ChatKind::Channel] {
        let mut snapshot = capabilities(42, chat_kind);
        snapshot.send_messages = CapabilityValue::Known(false);
        snapshot.send_media = CapabilityValue::Known(true);
        snapshot.post_messages = CapabilityValue::Known(true);
        snapshot.slow_mode_remaining_seconds = CapabilityValue::Known(Some(30));

        assert!(snapshot.validate().is_ok());
        assert!(!snapshot.allows(ChatAction::SendMessages));
        assert!(snapshot.allows(ChatAction::SendMedia));
        assert!(snapshot.allows(ChatAction::PostMessages));
    }
}

#[test]
fn validation_rejects_zero_chat_id_and_accepts_zero_remaining_slow_mode() {
    assert!(capabilities(0, ChatKind::Private).validate().is_err());

    let mut snapshot = capabilities(1, ChatKind::Supergroup);
    snapshot.slow_mode_remaining_seconds = CapabilityValue::Known(Some(0));
    assert!(snapshot.validate().is_ok());
}

#[test]
fn reducer_stores_valid_snapshots_and_rejects_invalid_ones() {
    let mut state = CoreState::default();
    let mut snapshot = capabilities(7, ChatKind::Channel);
    snapshot.post_messages = CapabilityValue::Known(true);

    state.reduce(&CoreEvent::ChatCapabilitiesUpdated(snapshot.clone()));
    assert_eq!(state.chat_capabilities.get(&7), Some(&snapshot));

    let mut invalid = capabilities(0, ChatKind::Group);
    invalid.add_users = CapabilityValue::Known(true);
    state.reduce(&CoreEvent::ChatCapabilitiesUpdated(invalid));
    assert!(!state.chat_capabilities.contains_key(&0));
    assert!(state.last_error.as_deref().unwrap().contains("chat id"));
}

#[test]
fn capabilities_and_events_round_trip_through_json() {
    let mut snapshot = capabilities(-100, ChatKind::Supergroup);
    snapshot.edit_messages = CapabilityValue::Known(true);
    snapshot.edit_others_messages = CapabilityValue::Known(false);
    snapshot.slow_mode_remaining_seconds = CapabilityValue::Known(None);
    assert!(snapshot.validate().is_ok());
    let event = CoreEvent::ChatCapabilitiesUpdated(snapshot.clone());

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "chat_capabilities_updated");
    let decoded: CoreEvent = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, event);

    let encoded = serde_json::to_string(&snapshot).unwrap();
    let decoded: ChatCapabilities = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, snapshot);
}

#[test]
fn old_state_json_defaults_the_new_capability_map() {
    let old_state = json!({
        "running": true,
        "account": { "authorization": "Closed", "chats": [] },
        "messages": {},
        "last_error": null
    });

    let decoded: CoreState = serde_json::from_value(old_state).unwrap();
    assert!(decoded.chat_capabilities.is_empty());
}
