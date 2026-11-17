use tg_mobile_core::{
    encode_event, encode_state, spawn_core_with_store, AuthorizationState, CoreEvent, CoreState,
    LoginCodeDeliveryMethod, MemoryStateStore, MockTelegramBackend, StateStore,
};
use tokio::time::{timeout, Duration};

#[test]
fn delivery_event_has_stable_wire_shape_and_is_transient() {
    let event =
        CoreEvent::AuthorizationCodeDeliveryChanged(Some(LoginCodeDeliveryMethod::TelegramApp));
    assert_eq!(event.kind(), "authorization_code_delivery_changed");
    assert!(event.is_transient());
    assert_eq!(
        encode_event(&event).unwrap(),
        r#"{"type":"authorization_code_delivery_changed","payload":"telegram_app"}"#
    );

    let cleared = CoreEvent::AuthorizationCodeDeliveryChanged(None);
    assert!(cleared.is_transient());
    assert_eq!(
        encode_event(&cleared).unwrap(),
        r#"{"type":"authorization_code_delivery_changed","payload":null}"#
    );
}

#[test]
fn reducer_does_not_persist_display_only_delivery_metadata() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::AuthorizationChanged(
        AuthorizationState::WaitingForCode,
    ));
    state.reduce(&CoreEvent::AuthorizationCodeDeliveryChanged(Some(
        LoginCodeDeliveryMethod::Sms,
    )));

    assert_eq!(
        state.account.authorization,
        AuthorizationState::WaitingForCode
    );
    let encoded_state = serde_json::to_string(&state).unwrap();
    assert!(!encoded_state.contains("login_code_delivery"));
    assert!(!encoded_state.contains("authorization_code_delivery_changed"));
    assert!(!encoded_state.contains("restore_complete"));
}

#[tokio::test]
async fn restore_complete_is_emitted_after_stored_ready_or_closed_state_loads() {
    for authorization in [AuthorizationState::Ready, AuthorizationState::Closed] {
        let store = MemoryStateStore::default();
        let mut persisted = CoreState::default();
        persisted.account.authorization = authorization.clone();
        store.save(&persisted).await.unwrap();
        assert!(!store.snapshot().unwrap().unwrap().restore_complete);

        let core = spawn_core_with_store(MockTelegramBackend::default(), store);
        let mut state = core.subscribe_state();
        timeout(Duration::from_secs(1), async {
            while !state.borrow().restore_complete {
                state.changed().await.unwrap();
            }
        })
        .await
        .unwrap();

        assert!(state.borrow().restore_complete);
        assert_eq!(state.borrow().account.authorization, authorization);
        core.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn empty_store_also_completes_restore_before_auth_ui_can_open() {
    let core = spawn_core_with_store(MockTelegramBackend::default(), MemoryStateStore::default());
    let mut state = core.subscribe_state();
    timeout(Duration::from_secs(1), async {
        while !state.borrow().restore_complete {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(
        state.borrow().account.authorization,
        AuthorizationState::Closed
    );
    assert!(state.borrow().restore_complete);
    assert!(encode_state(&state.borrow())
        .unwrap()
        .contains("\"restore_complete\":true"));
    core.shutdown().await.unwrap();
}
