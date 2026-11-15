use std::time::Duration;

use tg_mobile_core::{
    backend::{BackendFuture, BackendUpdateReceiver},
    spawn_core, AuthorizationState, CoreCommand, CoreError, CoreEvent, MessageSummary,
    TelegramBackend,
};
use tokio::{sync::mpsc, time::timeout};

struct UpdateBackend {
    updates: Option<BackendUpdateReceiver>,
}

impl TelegramBackend for UpdateBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async move {
            match command {
                CoreCommand::Start => Ok(vec![CoreEvent::AuthorizationChanged(
                    AuthorizationState::WaitingForPhoneNumber,
                )]),
                _ => Ok(Vec::new()),
            }
        })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        self.updates.take()
    }
}

fn message() -> MessageSummary {
    MessageSummary {
        id: 55,
        chat_id: 77,
        sender_id: 99,
        text: "incoming".into(),
        timestamp_unix: 123,
        outgoing: false,
        reactions: Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsolicited_backend_update_reaches_events_and_state() {
    let (updates_tx, updates_rx) = mpsc::channel(8);
    let core = spawn_core(UpdateBackend {
        updates: Some(updates_rx),
    });
    let mut events = core.subscribe();

    updates_tx
        .send(Ok(CoreEvent::MessageAdded(message())))
        .await
        .unwrap();

    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::MessageAdded(received) if received == message()
    ));

    let state = core.state();
    assert_eq!(state.messages_for_chat(77), &[message()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backend_update_error_becomes_core_error_event() {
    let (updates_tx, updates_rx) = mpsc::channel(8);
    let core = spawn_core(UpdateBackend {
        updates: Some(updates_rx),
    });
    let mut events = core.subscribe();

    updates_tx
        .send(Err(CoreError::Backend("update failed".into())))
        .await
        .unwrap();

    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::Error(message) if message.contains("update failed")
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closed_update_stream_does_not_stop_command_processing() {
    let (updates_tx, updates_rx) = mpsc::channel(1);
    let core = spawn_core(UpdateBackend {
        updates: Some(updates_rx),
    });
    let mut events = core.subscribe();

    drop(updates_tx);
    tokio::task::yield_now().await;

    core.send(CoreCommand::Start).await.unwrap();

    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::AuthorizationChanged(AuthorizationState::WaitingForPhoneNumber)
    ));
}
