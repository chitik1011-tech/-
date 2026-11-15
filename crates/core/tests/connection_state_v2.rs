use std::time::Duration;

use tg_mobile_core::{
    encode_event, spawn_core_with_store, AuthorizationState, ConnectionState, CoreEvent, CoreState,
    MemoryStateStore, MockTelegramBackend, StateStore,
};
use tokio::time::timeout;

#[test]
fn connection_event_has_stable_wire_shape_and_reduces_into_state() {
    let event = CoreEvent::ConnectionStateChanged(ConnectionState::Connecting);
    assert_eq!(event.kind(), "connection_state_changed");
    assert_eq!(
        encode_event(&event).unwrap(),
        r#"{"type":"connection_state_changed","payload":"connecting"}"#
    );

    let mut state = CoreState::default();
    assert_eq!(state.connection_state, ConnectionState::Disconnected);

    state.reduce(&event);
    assert_eq!(state.connection_state, ConnectionState::Connecting);

    state.reduce(&CoreEvent::ConnectionStateChanged(ConnectionState::Ready));
    assert_eq!(state.connection_state, ConnectionState::Ready);
}

#[test]
fn legacy_persisted_state_defaults_connection_to_disconnected() {
    let legacy = r#"{
        "running": true,
        "account": {"authorization":"Closed","chats":[]},
        "messages": {},
        "last_error": null
    }"#;

    let state: CoreState = serde_json::from_str(legacy).unwrap();
    assert_eq!(state.connection_state, ConnectionState::Disconnected);
}

#[tokio::test]
async fn restored_snapshot_never_reports_stale_ready_connection() {
    let store = MemoryStateStore::default();
    let mut persisted = CoreState::default();
    persisted.account.authorization = AuthorizationState::Ready;
    persisted.connection_state = ConnectionState::Ready;
    store.save(&persisted).await.unwrap();

    let core = spawn_core_with_store(MockTelegramBackend::default(), store);

    timeout(Duration::from_secs(1), async {
        loop {
            let state = core.state();
            if state.account.authorization == AuthorizationState::Ready {
                assert_eq!(state.connection_state, ConnectionState::Disconnected);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    core.shutdown().await.unwrap();
}

#[test]
fn connection_states_round_trip_through_json() {
    for state in [
        ConnectionState::Connecting,
        ConnectionState::Ready,
        ConnectionState::Disconnected,
    ] {
        let encoded = serde_json::to_string(&state).unwrap();
        let decoded: ConnectionState = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, state);
    }
}
