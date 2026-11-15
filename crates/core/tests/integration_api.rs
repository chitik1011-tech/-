use std::time::Duration;

use tg_mobile_core::{
    backend_update_channel, spawn_core, AuthorizationState, CoreCommand, CoreEvent,
    MockTelegramBackend, DEFAULT_BACKEND_UPDATE_CAPACITY,
};
use tokio::time::timeout;

#[test]
fn command_and_event_kinds_are_stable() {
    assert_eq!(CoreCommand::Start.kind(), "start");
    assert_eq!(
        CoreCommand::SetPhoneNumber("test".into()).kind(),
        "set_phone_number"
    );
    assert_eq!(CoreCommand::LoadChats { limit: 1 }.kind(), "load_chats");
    assert_eq!(
        CoreCommand::ResolvePeer {
            input: "@example".into()
        }
        .kind(),
        "resolve_peer"
    );
    assert_eq!(
        CoreCommand::SendText {
            chat_id: 1,
            text: "hello".into(),
        }
        .kind(),
        "send_text"
    );

    assert_eq!(
        CoreEvent::AuthorizationChanged(AuthorizationState::Ready).kind(),
        "authorization_changed"
    );
    assert_eq!(CoreEvent::Error("test".into()).kind(), "error");
    assert_eq!(
        CoreEvent::PeerResolved(tg_mobile_core::PeerResolution::NotFound).kind(),
        "peer_resolved"
    );
    assert_eq!(CoreEvent::Stopped.kind(), "stopped");
    assert!(CoreEvent::Stopped.is_terminal());
    assert!(!CoreEvent::Error("test".into()).is_terminal());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backend_update_channel_uses_default_capacity() {
    let (sender, mut receiver) = backend_update_channel();
    assert_eq!(sender.capacity(), DEFAULT_BACKEND_UPDATE_CAPACITY);

    sender.send(Ok(CoreEvent::Stopped)).await.unwrap();

    assert!(matches!(
        receiver.recv().await,
        Some(Ok(CoreEvent::Stopped))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_helper_stops_and_closes_runtime() {
    let core = spawn_core(MockTelegramBackend::default());
    let mut events = core.subscribe();

    core.shutdown().await.unwrap();

    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::Stopped
    ));

    timeout(Duration::from_secs(1), async {
        while !core.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
