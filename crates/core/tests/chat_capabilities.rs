use tg_mobile_core::{
    encode_event, encode_state, CapabilityValue as Value, ChatCapabilities, ChatKind, CoreEvent,
    CoreState,
};

fn capabilities(chat_id: i64) -> ChatCapabilities {
    let mut capabilities = ChatCapabilities {
        chat_id,
        chat_kind: ChatKind::Supergroup,
        can_send_messages: Value::Known(true),
        slow_mode_enabled: Value::Known(false),
        slow_mode_remaining_seconds: Value::Known(None),
        ..ChatCapabilities::default()
    };
    capabilities.refresh_derived();
    capabilities
}

#[test]
fn capability_event_reduces_into_the_single_state_map_and_wire() {
    let cap = capabilities(42);
    let event = CoreEvent::ChatCapabilitiesUpdated(cap.clone());
    let wire = encode_event(&event).unwrap();
    assert!(wire.contains("chat_capabilities_updated"));
    assert!(wire.contains("can_send_messages"));

    let mut state = CoreState::default();
    state.reduce(&event);
    assert_eq!(state.chat_capabilities.get(&42), Some(&cap));
    let state_json = encode_state(&state).unwrap();
    assert!(state_json.contains("chat_capabilities"));
    assert!(state_json.contains("slow_mode_enabled"));
}

#[test]
fn capability_cache_is_cleared_on_session_replacement_and_persistence() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ChatCapabilitiesUpdated(capabilities(42)));
    assert!(state.persistence_snapshot().chat_capabilities.is_empty());

    state.reduce(&CoreEvent::ConnectionSessionStarted);
    assert!(state.chat_capabilities.is_empty());
}

#[test]
fn invalid_capability_identity_is_rejected_from_state() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ChatCapabilitiesUpdated(
        ChatCapabilities::default(),
    ));
    assert!(state.chat_capabilities.is_empty());
}
