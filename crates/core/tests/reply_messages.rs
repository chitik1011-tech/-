use tg_mobile_core::{CoreCommand, CoreEvent, MockTelegramBackend, ReplyMetadata, TelegramBackend};

#[test]
fn reply_command_validates_and_round_trips_over_the_wire() {
    let command = CoreCommand::ReplyMessage {
        chat_id: -4,
        reply_to_message_id: 9,
        text: "hello".into(),
    };
    assert_eq!(command.kind(), "reply_message");
    assert!(command.validate().is_ok());
    assert!(CoreCommand::ReplyMessage {
        chat_id: 4,
        reply_to_message_id: 0,
        text: "hello".into(),
    }
    .validate()
    .is_err());

    let json = serde_json::to_string(&command).expect("serialize reply command");
    assert_eq!(tg_mobile_core::decode_command(&json).unwrap(), command);
}

#[tokio::test]
async fn mock_reply_preserves_target_metadata() {
    let mut backend = MockTelegramBackend::default();
    backend.handle(CoreCommand::Start).await.unwrap();
    backend
        .handle(CoreCommand::SetPhoneNumber("+10000000000".into()))
        .await
        .unwrap();
    backend
        .handle(CoreCommand::SubmitCode("12345".into()))
        .await
        .unwrap();

    let events = backend
        .handle(CoreCommand::ReplyMessage {
            chat_id: 2,
            reply_to_message_id: 1,
            text: "reply".into(),
        })
        .await
        .unwrap();
    let CoreEvent::MessageAdded(message) = &events[0] else {
        panic!("expected a message event");
    };
    assert_eq!(
        message.reply_to,
        Some(ReplyMetadata {
            message_id: 1,
            chat_id: Some(2),
            sender_id: None,
            sender_display_name: None,
        })
    );
}
