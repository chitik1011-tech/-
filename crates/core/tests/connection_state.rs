use serde_json::json;
use tg_mobile_core::{encode_event, ConnectionState, ConnectionStateMachine, CoreEvent, CoreState};

#[test]
fn connection_state_machine_accepts_connection_and_recovery_transitions() {
    let mut machine = ConnectionStateMachine::default();

    assert_eq!(machine.current(), ConnectionState::Offline);
    assert_eq!(
        machine.transition(ConnectionState::Connecting),
        Some(ConnectionState::Connecting)
    );
    assert_eq!(
        machine.transition(ConnectionState::Connected),
        Some(ConnectionState::Connected)
    );
    assert_eq!(
        machine.transition(ConnectionState::Reconnecting),
        Some(ConnectionState::Reconnecting)
    );
    assert_eq!(
        machine.transition(ConnectionState::Offline),
        Some(ConnectionState::Offline)
    );
    assert_eq!(
        machine.transition(ConnectionState::Connecting),
        Some(ConnectionState::Connecting)
    );
}

#[test]
fn direct_recovery_to_connected_is_valid() {
    let mut machine = ConnectionStateMachine::default();

    assert_eq!(
        machine.transition(ConnectionState::Connected),
        Some(ConnectionState::Connected)
    );
    assert_eq!(
        machine.transition(ConnectionState::Failed),
        Some(ConnectionState::Failed)
    );
    assert_eq!(
        machine.transition(ConnectionState::Connected),
        Some(ConnectionState::Connected)
    );
}

#[test]
fn duplicate_and_invalid_transitions_are_ignored() {
    let mut machine = ConnectionStateMachine::new(ConnectionState::Connected);

    assert_eq!(machine.transition(ConnectionState::Connected), None);
    assert_eq!(machine.current(), ConnectionState::Connected);
    assert_eq!(machine.transition(ConnectionState::Connecting), None);
    assert_eq!(machine.current(), ConnectionState::Connected);
    assert_eq!(
        machine.transition(ConnectionState::Failed),
        Some(ConnectionState::Failed)
    );
    assert_eq!(
        machine.transition(ConnectionState::Connecting),
        Some(ConnectionState::Connecting)
    );
}

#[test]
fn connection_event_uses_stable_json_wire_shape() {
    let event = CoreEvent::ConnectionChanged(ConnectionState::Reconnecting);
    let encoded = encode_event(&event).expect("connection event should serialize");
    let value: serde_json::Value =
        serde_json::from_str(&encoded).expect("wire event should be valid JSON");

    assert_eq!(event.kind(), "connection_changed");
    assert_eq!(
        value,
        json!({"type": "connection_changed", "payload": "reconnecting"})
    );
}

#[test]
fn reducer_tracks_connection_state_without_changing_auth_or_chat_data() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ConnectionChanged(ConnectionState::Connecting));
    assert_eq!(state.connection_state, ConnectionState::Connecting);

    state.reduce(&CoreEvent::ConnectionChanged(ConnectionState::Connected));
    assert_eq!(state.connection_state, ConnectionState::Connected);
    assert!(state.account.chats.is_empty());
}

#[test]
fn old_persisted_state_defaults_to_offline() {
    let legacy = json!({
        "running": true,
        "account": {"authorization": "Closed", "chats": []},
        "messages": {},
        "last_error": null
    });

    let state: CoreState = serde_json::from_value(legacy)
        .expect("pre-connection-state CoreState should still deserialize");
    assert_eq!(state.connection_state, ConnectionState::Offline);
}
