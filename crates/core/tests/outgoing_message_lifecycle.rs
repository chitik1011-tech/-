use std::time::Duration;

use tg_mobile_core::{
    CoreCommand, CoreEvent, MockTelegramBackend, OutgoingMessageCommand, OutgoingMessageStatus,
    spawn_core,
};
use tokio::time::timeout;

async fn authorize(core: &tg_mobile_core::CoreHandle) {
    core.send(CoreCommand::Start).await.unwrap();
    core.send(CoreCommand::SetPhoneNumber("+15550001111".into()))
        .await
        .unwrap();
    core.send(CoreCommand::SubmitCode("12345".into()))
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
    let statuses =
        wait_for_status(&mut events, "pending-7d2a", OutgoingMessageStatus::Sent).await;

    assert_eq!(
        statuses,
        vec![
            OutgoingMessageStatus::Draft,
            OutgoingMessageStatus::Sending,
            OutgoingMessageStatus::Sent,
        ]
    );
    let sent = core.state().outgoing_message("pending-7d2a").unwrap().clone();
    assert_eq!(sent.status, OutgoingMessageStatus::Sent);
    assert!(sent.server_message_id.is_some());
    assert_eq!(core.state().messages_for_chat(2).len(), 1);

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
        core.state().outgoing_message("pending-7d2a").unwrap().status,
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
    let failed = core.state().outgoing_message("pending-7d2a").unwrap();
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
