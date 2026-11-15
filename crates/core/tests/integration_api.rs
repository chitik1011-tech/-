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
        CoreCommand::LoadPinnedMessage { chat_id: 2 }.kind(),
        "load_pinned_message"
    );
    assert_eq!(
        CoreCommand::PinMessage {
            chat_id: 2,
            message_id: 4
        }
        .kind(),
        "pin_message"
    );
    assert_eq!(
        CoreCommand::UnpinMessage { chat_id: 2 }.kind(),
        "unpin_message"
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
    assert_eq!(
        CoreEvent::PinnedMessageUpdated {
            chat_id: 2,
            message: None
        }
        .kind(),
        "pinned_message_updated"
    );
    assert_eq!(CoreEvent::Error("test".into()).kind(), "error");
    assert_eq!(CoreEvent::Stopped.kind(), "stopped");
    assert!(CoreEvent::Stopped.is_terminal());
    assert!(!CoreEvent::PinnedMessageUpdated {
        chat_id: 2,
        message: None
    }
    .is_terminal());
    assert!(!CoreEvent::Error("test".into()).is_terminal());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_backend_loads_pins_and_supports_pin_unpin() {
    let core = spawn_core(MockTelegramBackend::default());
    let mut events = core.subscribe();

    core.send(CoreCommand::PinMessage {
        chat_id: 2,
        message_id: 1,
    })
    .await
    .unwrap();
    let pinned_event = timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        pinned_event,
        CoreEvent::PinnedMessageUpdated {
            chat_id: 2,
            message: Some(ref message)
        } if message.id == 1
    ));
    assert_eq!(
        core.state()
            .chat(2)
            .unwrap()
            .pinned_message
            .as_ref()
            .unwrap()
            .id,
        1
    );

    core.send(CoreCommand::LoadPinnedMessage { chat_id: 2 })
        .await
        .unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv()).await.unwrap().unwrap(),
        CoreEvent::PinnedMessageUpdated {
            chat_id: 2,
            message: Some(ref message)
        } if message.id == 1
    ));

    core.send(CoreCommand::UnpinMessage { chat_id: 2 })
        .await
        .unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv()).await.unwrap().unwrap(),
        CoreEvent::PinnedMessageUpdated { chat_id: 2, message: None }
    ));
    assert_eq!(core.state().chat(2).unwrap().pinned_message, None);

    core.shutdown().await.unwrap();
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
