use std::time::Duration;

use tg_mobile_core::{
    spawn_core, spawn_core_with_store, CoreCommand, CoreEvent, CoreState, MemoryStateStore,
    MockTelegramBackend, OutgoingMessage, OutgoingMessageCommand, OutgoingMessageStatus,
    StateStore,
};
use tokio::time::timeout;

async fn authorize(core: &tg_mobile_core::CoreHandle) {
    core.send(CoreCommand::Start).await.unwrap();
    core.send(CoreCommand::SetPhoneNumber("test-phone".into()))
        .await
        .unwrap();
    core.send(CoreCommand::SubmitCode("test-code".into()))
        .await
        .unwrap();
}

async fn wait_for_status(
    events: &mut tokio::sync::broadcast::Receiver<CoreEvent>,
    client_id: &str,
    target: OutgoingMessageStatus,
) -> Vec<OutgoingMessageStatus> {
    timeout(Duration::from_secs(3), async {
        let mut statuses = Vec::new();
        loop {
            let event = events.recv().await.expect("event stream should stay open");
            if let CoreEvent::OutgoingMessageChanged(message) = event {
                if message.client_id == client_id {
                    statuses.push(message.status);
                    if message.status == target {
                        return statuses;
                    }
                }
            }
        }
    })
    .await
    .expect("outgoing message lifecycle should reach its expected state")
}

fn request() -> OutgoingMessageCommand {
    OutgoingMessageCommand::SendTextWithId {
        client_id: "pending-7d2a".into(),
        chat_id: 2,
        text: "hello from the outbox".into(),
    }
}

#[tokio::test]
async fn successful_send_emits_draft_sending_sent_and_is_idempotent() {
    let core = spawn_core(MockTelegramBackend::default());
    let mut events = core.subscribe();
    authorize(&core).await;

    let request = request();
    core.send_outgoing(request.clone()).await.unwrap();
    let statuses = wait_for_status(&mut events, "pending-7d2a", OutgoingMessageStatus::Sent).await;

    assert_eq!(
        statuses,
        vec![
            OutgoingMessageStatus::Draft,
            OutgoingMessageStatus::Sending,
            OutgoingMessageStatus::Sent,
        ]
    );
    let sent = core
        .state()
        .outgoing_message("pending-7d2a")
        .unwrap()
        .clone();
    assert_eq!(sent.status, OutgoingMessageStatus::Sent);
    let server_id = sent
        .server_message_id
        .expect("mock should return server message ID");
    let state = core.state();
    assert_eq!(state.messages_for_chat(2).len(), 1);
    assert!(state
        .messages_for_chat(2)
        .iter()
        .any(|message| message.id == server_id));
    assert!(
        state.visible_outgoing_messages_for_chat(2).is_empty(),
        "the accepted outbox status stays queryable without rendering a duplicate row"
    );

    // Replaying the same stable ID after success must not call the backend twice.
    core.send_outgoing(request).await.unwrap();
    core.send(CoreCommand::LoadChats { limit: 100 })
        .await
        .unwrap();
    timeout(Duration::from_secs(3), async {
        loop {
            if matches!(events.recv().await, Ok(CoreEvent::ChatsLoaded(_))) {
                break;
            }
        }
    })
    .await
    .expect("barrier command should be handled");
    assert_eq!(core.state().messages_for_chat(2).len(), 1);
    assert_eq!(
        core.state()
            .outgoing_message("pending-7d2a")
            .unwrap()
            .status,
        OutgoingMessageStatus::Sent
    );
}

#[tokio::test]
async fn failed_send_can_retry_with_the_same_stable_id() {
    let backend = MockTelegramBackend::default().with_next_send_failure("network timeout");
    let core = spawn_core(backend);
    let mut events = core.subscribe();
    authorize(&core).await;

    let request = request();
    core.send_outgoing(request.clone()).await.unwrap();
    let failed_statuses =
        wait_for_status(&mut events, "pending-7d2a", OutgoingMessageStatus::Failed).await;
    assert_eq!(
        failed_statuses,
        vec![
            OutgoingMessageStatus::Draft,
            OutgoingMessageStatus::Sending,
            OutgoingMessageStatus::Failed,
        ]
    );
    let snapshot = core.state();
    let failed = snapshot.outgoing_message("pending-7d2a").unwrap();
    assert_eq!(failed.status, OutgoingMessageStatus::Failed);
    assert!(failed.error.as_deref().unwrap().contains("network timeout"));

    // Retrying reuses the same logical ID; core transitions Failed -> Sending -> Sent.
    core.send_outgoing(request).await.unwrap();
    let retry_statuses =
        wait_for_status(&mut events, "pending-7d2a", OutgoingMessageStatus::Sent).await;
    assert_eq!(
        retry_statuses,
        vec![OutgoingMessageStatus::Sending, OutgoingMessageStatus::Sent]
    );
    assert_eq!(core.state().messages_for_chat(2).len(), 1);
}

#[tokio::test]
async fn interrupted_sending_state_is_restored_as_retryable_failure() {
    let mut initial = CoreState::default();
    let draft = OutgoingMessage {
        client_id: "pending-after-restart".into(),
        chat_id: 2,
        text: "hello".into(),
        status: OutgoingMessageStatus::Draft,
        server_message_id: None,
        error: None,
    };
    initial.reduce(&CoreEvent::OutgoingMessageChanged(draft.clone()));
    let mut sending = draft;
    sending.status = OutgoingMessageStatus::Sending;
    initial.reduce(&CoreEvent::OutgoingMessageChanged(sending));

    let store = MemoryStateStore::default();
    store.save(&initial).await.unwrap();
    let core = spawn_core_with_store(MockTelegramBackend::default(), store.clone());
    let mut state_updates = core.subscribe_state();

    let restored = timeout(Duration::from_secs(3), async {
        loop {
            let snapshot = core.state();
            if snapshot
                .outgoing_message("pending-after-restart")
                .is_some_and(|message| message.status == OutgoingMessageStatus::Failed)
            {
                break snapshot;
            }
            state_updates
                .changed()
                .await
                .expect("core state watcher should remain open");
        }
    })
    .await
    .expect("interrupted send should be recoverable");

    let message = restored.outgoing_message("pending-after-restart").unwrap();
    assert_eq!(message.status, OutgoingMessageStatus::Failed);
    assert!(message
        .error
        .as_deref()
        .unwrap()
        .contains("retrying may duplicate"));
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .unwrap()
            .outgoing_message("pending-after-restart")
            .unwrap()
            .status,
        OutgoingMessageStatus::Failed
    );
}
