use tg_mobile_core::{
    connection::ConnectionState,
    connection_machine::{ConnectionMachine, LinkError, LinkEvent, LinkPhase, Outcome},
};

fn changed(from: LinkPhase, to: LinkPhase) -> Outcome {
    Outcome::Changed { from, to }
}

fn sync_to_ready(machine: &mut ConnectionMachine) {
    let token = machine.token();
    machine.apply(token, LinkEvent::TransportConnected).unwrap();
    machine.apply(token, LinkEvent::SyncStarted).unwrap();
    machine.apply(token, LinkEvent::SyncCompleted).unwrap();
}

#[test]
fn normal_connect_sync_ready() {
    let mut machine = ConnectionMachine::default();
    let started = machine.start_session();
    assert_eq!(
        started,
        changed(LinkPhase::Disconnected, LinkPhase::Connecting)
    );

    let token = machine.token();
    let connected = machine.apply(token, LinkEvent::TransportConnected);
    assert_eq!(
        connected,
        Ok(changed(LinkPhase::Connecting, LinkPhase::Connected))
    );
    let syncing = machine.apply(token, LinkEvent::SyncStarted);
    assert_eq!(
        syncing,
        Ok(changed(LinkPhase::Connected, LinkPhase::Synchronizing))
    );
    let ready = machine.apply(token, LinkEvent::SyncCompleted);
    assert_eq!(
        ready,
        Ok(changed(LinkPhase::Synchronizing, LinkPhase::Ready))
    );
    assert_eq!(machine.transport_state(), ConnectionState::Ready);
}

#[test]
fn disconnect_then_reconnect_returns_to_ready() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    sync_to_ready(&mut machine);

    let ready_token = machine.token();
    let lost = machine.apply(ready_token, LinkEvent::TransportLost);
    assert_eq!(lost, Ok(changed(LinkPhase::Ready, LinkPhase::Reconnecting)));
    assert_eq!(machine.transport_state(), ConnectionState::Disconnected);

    let backoff_token = machine.token();
    let retry = machine.apply(backoff_token, LinkEvent::Retry);
    assert_eq!(
        retry,
        Ok(changed(LinkPhase::Reconnecting, LinkPhase::Connecting))
    );
    sync_to_ready(&mut machine);
    assert_eq!(machine.phase(), LinkPhase::Ready);
    assert_eq!(machine.transport_state(), ConnectionState::Ready);
}

#[test]
fn callback_from_previous_generation_cannot_touch_new_session() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    let token_a = machine.token();
    machine.apply(token_a, LinkEvent::Logout).unwrap();

    machine.start_session();
    assert_ne!(machine.token().generation(), token_a.generation());

    let late = machine.apply(token_a, LinkEvent::TransportConnected);
    assert_eq!(late, Ok(Outcome::Stale));
    assert_eq!(machine.phase(), LinkPhase::Connecting);
}

#[test]
fn late_disconnect_from_old_generation_is_ignored_after_ready() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    let token_a = machine.token();
    machine.apply(token_a, LinkEvent::Logout).unwrap();

    machine.start_session();
    sync_to_ready(&mut machine);

    let late = machine.apply(token_a, LinkEvent::TransportLost);
    assert_eq!(late, Ok(Outcome::Stale));
    assert_eq!(machine.phase(), LinkPhase::Ready);
}

#[test]
fn duplicate_ready_is_a_noop() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    sync_to_ready(&mut machine);

    let token = machine.token();
    let duplicate = machine.apply(token, LinkEvent::SyncCompleted);
    assert_eq!(duplicate, Ok(Outcome::Duplicate));
    assert_eq!(machine.phase(), LinkPhase::Ready);
}

#[test]
fn retryable_failure_then_reconnect() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    let first_attempt = machine.token();

    let failed = machine.apply(first_attempt, LinkEvent::ConnectFailed { retryable: true });
    assert_eq!(
        failed,
        Ok(changed(LinkPhase::Connecting, LinkPhase::Reconnecting))
    );

    let backoff_token = machine.token();
    assert_ne!(backoff_token, first_attempt);

    let stale_retry = machine.apply(first_attempt, LinkEvent::Retry);
    assert_eq!(stale_retry, Ok(Outcome::Stale));

    let retry = machine.apply(backoff_token, LinkEvent::Retry);
    assert_eq!(
        retry,
        Ok(changed(LinkPhase::Reconnecting, LinkPhase::Connecting))
    );
    sync_to_ready(&mut machine);
    assert_eq!(machine.phase(), LinkPhase::Ready);
}

#[test]
fn fatal_failure_then_manual_retry() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    let token = machine.token();

    let fatal = machine.apply(token, LinkEvent::ConnectFailed { retryable: false });
    assert_eq!(fatal, Ok(changed(LinkPhase::Connecting, LinkPhase::Failed)));
    assert_eq!(machine.transport_state(), ConnectionState::Disconnected);

    let failed_token = machine.token();
    let manual = machine.apply(failed_token, LinkEvent::Retry);
    assert_eq!(
        manual,
        Ok(changed(LinkPhase::Failed, LinkPhase::Connecting))
    );
}

#[test]
fn logout_then_login_creates_new_generation() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    sync_to_ready(&mut machine);
    let token_a = machine.token();

    let logout = machine.apply(token_a, LinkEvent::Logout);
    assert_eq!(
        logout,
        Ok(changed(LinkPhase::Ready, LinkPhase::Disconnected))
    );

    let after_logout = machine.token();
    let retry = machine.apply(after_logout, LinkEvent::Retry);
    let expected = Err(LinkError::InvalidTransition {
        from: LinkPhase::Disconnected,
        event: LinkEvent::Retry,
    });
    assert_eq!(retry, expected);

    let login = machine.start_session();
    assert_eq!(
        login,
        changed(LinkPhase::Disconnected, LinkPhase::Connecting)
    );
    assert!(machine.token().generation() > token_a.generation());

    sync_to_ready(&mut machine);
    assert_eq!(machine.phase(), LinkPhase::Ready);
}

#[test]
fn stale_async_result_after_loss_is_rejected() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    let attempt = machine.token();
    machine
        .apply(attempt, LinkEvent::TransportConnected)
        .unwrap();
    machine.apply(attempt, LinkEvent::SyncStarted).unwrap();

    let in_flight_sync = machine.token();
    let lost = machine.apply(in_flight_sync, LinkEvent::TransportLost);
    assert_eq!(
        lost,
        Ok(changed(LinkPhase::Synchronizing, LinkPhase::Reconnecting))
    );

    let late_result = machine.apply(in_flight_sync, LinkEvent::SyncCompleted);
    assert_eq!(late_result, Ok(Outcome::Stale));
    assert_eq!(machine.phase(), LinkPhase::Reconnecting);
}

#[test]
fn shutdown_during_reconnect_cancels_pending_retry() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    let attempt = machine.token();
    machine
        .apply(attempt, LinkEvent::ConnectFailed { retryable: true })
        .unwrap();

    let backoff_token = machine.token();
    let shutdown = machine.apply(backoff_token, LinkEvent::Shutdown);
    assert_eq!(
        shutdown,
        Ok(changed(LinkPhase::Reconnecting, LinkPhase::Disconnected))
    );

    let timer_retry = machine.apply(backoff_token, LinkEvent::Retry);
    assert_eq!(timer_retry, Ok(Outcome::Stale));

    let current = machine.token();
    let after_shutdown = machine.apply(current, LinkEvent::Retry);
    let expected = Err(LinkError::InvalidTransition {
        from: LinkPhase::Disconnected,
        event: LinkEvent::Retry,
    });
    assert_eq!(after_shutdown, expected);
    assert_eq!(machine.phase(), LinkPhase::Disconnected);
}

#[test]
fn invalid_transition_is_rejected_without_state_change() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    let token = machine.token();

    let result = machine.apply(token, LinkEvent::SyncCompleted);
    let expected = Err(LinkError::InvalidTransition {
        from: LinkPhase::Connecting,
        event: LinkEvent::SyncCompleted,
    });
    assert_eq!(result, expected);
    assert_eq!(machine.phase(), LinkPhase::Connecting);
    assert_eq!(machine.token(), token);
}

#[test]
fn start_session_is_idempotent_while_active() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    let token = machine.token();

    assert_eq!(machine.start_session(), Outcome::Duplicate);
    assert_eq!(machine.token(), token);

    sync_to_ready(&mut machine);
    assert_eq!(machine.start_session(), Outcome::Duplicate);
    assert_eq!(machine.phase(), LinkPhase::Ready);
}

#[test]
fn stale_token_never_changes_phase_even_for_impossible_event() {
    let mut machine = ConnectionMachine::default();
    machine.start_session();
    let old = machine.token();
    machine.apply(old, LinkEvent::Logout).unwrap();
    machine.start_session();

    let result = machine.apply(old, LinkEvent::Retry);
    assert_eq!(result, Ok(Outcome::Stale));
    assert_eq!(machine.phase(), LinkPhase::Connecting);
}
