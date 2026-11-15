use std::time::Duration;

use tg_mobile_core::{
    spawn_core, AuthorizationState, CoreCommand, CoreEvent, CoreState, MockTelegramBackend,
};
use tokio::time::timeout;

#[test]
fn reducer_updates_authorization_and_errors() {
    let mut state = CoreState::default();

    state.reduce(&CoreEvent::AuthorizationChanged(
        AuthorizationState::WaitingForPhoneNumber,
    ));
    assert_eq!(
        state.account.authorization,
        AuthorizationState::WaitingForPhoneNumber
    );

    state.reduce(&CoreEvent::Error("boom".into()));
    assert_eq!(state.last_error.as_deref(), Some("boom"));

    state.reduce(&CoreEvent::Stopped);
    assert!(!state.running);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_backend_can_authorize_load_chats_and_send() {
    let core = spawn_core(MockTelegramBackend::default());
    let mut events = core.subscribe();

    core.send(CoreCommand::Start).await.unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::AuthorizationChanged(AuthorizationState::WaitingForPhoneNumber)
    ));

    core.send(CoreCommand::SetPhoneNumber("+10000000000".into()))
        .await
        .unwrap();
    let _ = timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();

    core.send(CoreCommand::SubmitCode("12345".into()))
        .await
        .unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::AuthorizationChanged(AuthorizationState::Ready)
    ));

    core.send(CoreCommand::LoadChats { limit: 10 })
        .await
        .unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::ChatsLoaded(chats) if !chats.is_empty()
    ));

    core.send(CoreCommand::SendText {
        chat_id: 1,
        text: "hello from rust".into(),
    })
    .await
    .unwrap();

    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::MessageAdded(message) if message.text == "hello from rust"
    ));
}
