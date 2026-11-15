use tg_mobile_core::{
    CoreCommand, CoreEvent, CoreState, MockTelegramBackend, OutgoingSend, OutgoingSendState,
    TelegramBackend, decode_command,
};

async fn authorize(backend: &mut MockTelegramBackend) {
    backend.handle(CoreCommand::Start).await.unwrap();
    backend
        .handle(CoreCommand::SetPhoneNumber("+10000000000".into()))
        .await
        .unwrap();
    backend
        .handle(CoreCommand::SubmitCode("12345".into()))
        .await
        .unwrap();
}

#[tokio::test]
async fn retrying_same_client_id_does_not_add_a_second_message() {
    let mut backend = MockTelegramBackend::default();
    authorize(&mut backend).await;

    let command = CoreCommand::SendTextReliable {
        client_id: 991,
        chat_id: 2,
        text: "hello".into(),
    };

    let first = backend.handle(command.clone()).await.unwrap();
    let first_message_id = first
        .iter()
        .find_map(|event| match event {
            CoreEvent::MessageAdded(message) => Some(message.id),
            _ => None,
        })
        .expect("first send must add the message");

    assert!(first.iter().any(|event| matches!(
        event,
        CoreEvent::MessageSendStateChanged(OutgoingSend {
            state: OutgoingSendState::Sent { message_id },
            ..
        }) if *message_id == first_message_id
    )));

    let retry = backend.handle(command).await.unwrap();
    assert!(
        !retry
            .iter()
            .any(|event| matches!(event, CoreEvent::MessageAdded(_))),
        "an idempotent retry must not add a duplicate message"
    );
    assert!(retry.iter().any(|event| matches!(
        event,
        CoreEvent::MessageSendStateChanged(OutgoingSend {
            state: OutgoingSendState::Sent { message_id },
            ..
        }) if *message_id == first_message_id
    )));
}

#[tokio::test]
async fn reusing_client_id_for_different_content_is_rejected_without_sending() {
    let mut backend = MockTelegramBackend::default();
    authorize(&mut backend).await;

    backend
        .handle(CoreCommand::SendTextReliable {
            client_id: 7,
            chat_id: 2,
            text: "first".into(),
        })
        .await
        .unwrap();

    let events = backend
        .handle(CoreCommand::SendTextReliable {
            client_id: 7,
            chat_id: 2,
            text: "different".into(),
        })
        .await
        .unwrap();

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, CoreEvent::MessageAdded(_)))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        CoreEvent::MessageSendStateChanged(OutgoingSend {
            state: OutgoingSendState::Failed {
                retryable: false,
                ..
            },
            ..
        })
    )));
}

#[test]
fn core_state_tracks_retryable_outbox_and_removes_success() {
    let mut state = CoreState::default();

    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
        101, 2, "hello",
    )));
    assert!(matches!(
        state.outgoing_send(101).map(|send| &send.state),
        Some(OutgoingSendState::Sending)
    ));

    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::failed(
        101,
        2,
        "hello",
        "network unavailable",
        true,
    )));
    assert!(matches!(
        state.outgoing_send(101).map(|send| &send.state),
        Some(OutgoingSendState::Failed {
            retryable: true,
            ..
        })
    ));

    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
        101, 2, "hello",
    )));
    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sent(
        101, 2, "hello", 55,
    )));
    assert!(state.outgoing_send(101).is_none());
}

#[test]
fn core_state_rejects_client_id_collision_while_request_is_in_outbox() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
        15, 2, "first",
    )));
    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
        15, 2, "different",
    )));

    assert_eq!(state.outgoing_send(15).unwrap().text, "first");
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|message| message.contains("reused"))
    );
}

#[test]
fn old_persisted_state_without_outbox_still_deserializes() {
    let json = r#"{
        "running": true,
        "account": {"authorization":"Closed","chats":[]},
        "messages": {},
        "last_error": null
    }"#;

    let state: CoreState = serde_json::from_str(json).unwrap();
    assert!(state.outbox.is_empty());
}

#[test]
fn reliable_send_wire_command_is_additive_and_validated() {
    let legacy = decode_command(
        r#"{"type":"send_text","payload":{"chat_id":2,"text":"legacy"}}"#,
    )
    .unwrap();
    assert_eq!(legacy.kind(), "send_text");

    let reliable = decode_command(
        r#"{"type":"send_text_reliable","payload":{"client_id":42,"chat_id":2,"text":"hello"}}"#,
    )
    .unwrap();
    assert_eq!(reliable.kind(), "send_text_reliable");
    assert!(reliable.validate().is_ok());

    let invalid = CoreCommand::SendTextReliable {
        client_id: 0,
        chat_id: 2,
        text: "hello".into(),
    };
    assert!(invalid.validate().is_err());
}
