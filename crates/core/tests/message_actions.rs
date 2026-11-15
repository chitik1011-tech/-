use tg_mobile_core::{
    decode_command, encode_event, CoreCommand, CoreEvent, CoreState, MessageSummary,
    MockTelegramBackend, TelegramBackend, MAX_MESSAGE_TEXT_BYTES,
};

#[test]
fn message_action_commands_round_trip_through_wire() {
    let commands = [
        CoreCommand::ReplyMessage {
            chat_id: 2,
            reply_to_message_id: 7,
            text: "reply".into(),
        },
        CoreCommand::EditMessage {
            chat_id: 2,
            message_id: 7,
            text: "edited".into(),
        },
        CoreCommand::DeleteMessage {
            chat_id: 2,
            message_id: 7,
        },
    ];

    for command in commands {
        let encoded = serde_json::to_string(&command).expect("encode command");
        assert_eq!(decode_command(&encoded).expect("decode command"), command);
    }

    let replied = CoreEvent::MessageReplied {
        message: message(2, 8, "reply", true, 4),
        reply_to_message_id: 7,
    };
    let encoded = encode_event(&replied).expect("encode reply event");
    assert_eq!(
        serde_json::from_str::<CoreEvent>(&encoded).expect("decode reply event"),
        replied
    );
    assert_eq!(replied.kind(), "message_replied");

    let edited = CoreEvent::MessageEdited(message(2, 7, "updated", true, 5));
    let encoded = encode_event(&edited).expect("encode edited event");
    assert_eq!(
        serde_json::from_str::<CoreEvent>(&encoded).expect("decode edited event"),
        edited
    );

    let deleted = CoreEvent::MessageDeleted {
        chat_id: 2,
        message_id: 7,
    };
    let encoded = encode_event(&deleted).expect("encode deleted event");
    assert_eq!(
        serde_json::from_str::<CoreEvent>(&encoded).expect("decode deleted event"),
        deleted
    );
}

#[test]
fn message_action_validation_rejects_missing_ids_blank_content_and_oversize_text() {
    let invalid = [
        CoreCommand::ReplyMessage {
            chat_id: 0,
            reply_to_message_id: 7,
            text: "reply".into(),
        },
        CoreCommand::ReplyMessage {
            chat_id: 2,
            reply_to_message_id: 0,
            text: "reply".into(),
        },
        CoreCommand::ReplyMessage {
            chat_id: -2,
            reply_to_message_id: -7,
            text: "reply".into(),
        },
        CoreCommand::EditMessage {
            chat_id: 2,
            message_id: -7,
            text: "edited".into(),
        },
        CoreCommand::DeleteMessage {
            chat_id: 2,
            message_id: -7,
        },
        CoreCommand::ReplyMessage {
            chat_id: 2,
            reply_to_message_id: 7,
            text: "  ".into(),
        },
        CoreCommand::EditMessage {
            chat_id: 2,
            message_id: 0,
            text: "edited".into(),
        },
        CoreCommand::EditMessage {
            chat_id: 2,
            message_id: 7,
            text: "\t ".into(),
        },
        CoreCommand::DeleteMessage {
            chat_id: 0,
            message_id: 7,
        },
        CoreCommand::DeleteMessage {
            chat_id: 2,
            message_id: 0,
        },
        CoreCommand::EditMessage {
            chat_id: 2,
            message_id: 7,
            text: "x".repeat(MAX_MESSAGE_TEXT_BYTES + 1),
        },
    ];

    for command in invalid {
        assert!(command.validate().is_err(), "accepted {command:?}");
    }
    assert!(CoreCommand::DeleteMessage {
        chat_id: -2,
        message_id: 7,
    }
    .validate()
    .is_ok());
}

#[tokio::test]
async fn mock_backend_replies_edits_and_deletes_by_chat_and_message_id() {
    let mut backend = MockTelegramBackend::default();
    send(&mut backend, CoreCommand::Start).await;
    send(
        &mut backend,
        CoreCommand::SetPhoneNumber("+10000000000".into()),
    )
    .await;
    send(&mut backend, CoreCommand::SubmitCode("12345".into())).await;

    let reply = send(
        &mut backend,
        CoreCommand::ReplyMessage {
            chat_id: 2,
            reply_to_message_id: 1,
            text: "a reply".into(),
        },
    )
    .await;
    let reply_message = match &reply[..] {
        [CoreEvent::MessageReplied {
            message,
            reply_to_message_id,
        }] => {
            assert_eq!(*reply_to_message_id, 1);
            message.clone()
        }
        other => panic!("unexpected reply events: {other:?}"),
    };
    assert_eq!(reply_message.chat_id, 2);
    assert_eq!(reply_message.id, 2);
    assert!(reply_message.outgoing);

    let edit = send(
        &mut backend,
        CoreCommand::EditMessage {
            chat_id: 2,
            message_id: reply_message.id,
            text: "edited reply".into(),
        },
    )
    .await;
    assert!(matches!(
        edit.as_slice(),
        [CoreEvent::MessageEdited(message)] if message.id == reply_message.id
            && message.chat_id == 2
            && message.text == "edited reply"
    ));

    let delete = send(
        &mut backend,
        CoreCommand::DeleteMessage {
            chat_id: 2,
            message_id: reply_message.id,
        },
    )
    .await;
    assert_eq!(
        delete,
        vec![CoreEvent::MessageDeleted {
            chat_id: 2,
            message_id: reply_message.id,
        }]
    );
}

#[test]
fn reducer_tracks_reply_targets_and_removes_deleted_reply_metadata() {
    let mut state = CoreState::default();
    let reply = message(2, 8, "reply", true, 11);
    state.reduce(&CoreEvent::MessageReplied {
        message: reply.clone(),
        reply_to_message_id: 7,
    });
    assert_eq!(state.messages_for_chat(2), &[reply]);
    assert_eq!(state.reply_target_for_message(2, 8), Some(7));
    let persisted = serde_json::to_string(&state).expect("serialize reply state");
    let restored: CoreState =
        serde_json::from_str(&persisted).expect("deserialize reply state");
    assert_eq!(restored.reply_target_for_message(2, 8), Some(7));

    state.reduce(&CoreEvent::MessageDeleted {
        chat_id: 2,
        message_id: 8,
    });
    assert_eq!(state.reply_target_for_message(2, 8), None);
}

#[test]
fn reducer_updates_edited_message_and_removes_deleted_message() {
    let original = message(2, 7, "before", false, 10);
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageAdded(original.clone()));

    let edited = message(2, 7, "after", false, 10);
    state.reduce(&CoreEvent::MessageEdited(edited.clone()));
    assert_eq!(state.messages_for_chat(2), &[edited.clone()]);
    assert_eq!(
        state.chat(2).and_then(|chat| chat.last_message.as_ref()),
        Some(&edited)
    );

    state.reduce(&CoreEvent::MessageDeleted {
        chat_id: 2,
        message_id: 7,
    });
    assert!(state.messages_for_chat(2).is_empty());
    assert!(state
        .chat(2)
        .is_some_and(|chat| chat.last_message.is_none()));
}

async fn send(backend: &mut MockTelegramBackend, command: CoreCommand) -> Vec<CoreEvent> {
    backend.handle(command).await.expect("mock command")
}

fn message(
    chat_id: i64,
    id: i64,
    text: &str,
    outgoing: bool,
    timestamp_unix: i64,
) -> MessageSummary {
    MessageSummary {
        id,
        chat_id,
        sender_id: if outgoing { 0 } else { 42 },
        text: text.into(),
        timestamp_unix,
        outgoing,
    }
}

#[test]
fn legacy_persisted_core_state_defaults_reply_targets() {
    let legacy = r#"{"running":true,"account":{"authorization":"Closed","chats":[]},"messages":{},"last_error":null}"#;
    let state: CoreState = serde_json::from_str(legacy)
        .expect("pre-reply CoreState should still deserialize");
    assert!(state.reply_targets.is_empty());
}
