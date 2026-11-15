use tg_mobile_core::{
    decode_command, ChatSummary, CoreCommand, CoreError, CoreEvent, CoreState, MessageSummary,
};

fn last_message(id: i64) -> MessageSummary {
    MessageSummary {
        id,
        chat_id: 77,
        sender_id: 12,
        text: format!("message-{id}"),
        timestamp_unix: id,
        outgoing: false,
        attachments: Vec::new(),
        forwarded_from: None,
        edited: false,
        edit_timestamp_unix: None,
        service_message: false,
        media_caption: None,
        reply_to: None,
    }
}

#[test]
fn mark_read_command_validates_ids() {
    assert!(CoreCommand::MarkRead {
        chat_id: 77,
        max_message_id: 10,
    }
    .validate()
    .is_ok());

    assert!(matches!(
        CoreCommand::MarkRead {
            chat_id: 0,
            max_message_id: 10,
        }
        .validate(),
        Err(CoreError::InvalidInput(_))
    ));

    assert!(matches!(
        CoreCommand::MarkRead {
            chat_id: 77,
            max_message_id: 0,
        }
        .validate(),
        Err(CoreError::InvalidInput(_))
    ));
}

#[test]
fn mark_read_wire_format_is_canonical() {
    let command =
        decode_command(r#"{"type":"mark_read","payload":{"chat_id":77,"max_message_id":42}}"#)
            .unwrap();

    assert_eq!(
        command,
        CoreCommand::MarkRead {
            chat_id: 77,
            max_message_id: 42,
        }
    );
    assert_eq!(command.kind(), "mark_read");
}

#[test]
fn reducer_only_clears_unread_when_receipt_covers_latest_message() {
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        id: 77,
        title: "chat".into(),
        unread_count: 4,
        last_message: Some(last_message(10)),
    });

    state.reduce(&CoreEvent::ChatRead {
        chat_id: 77,
        max_message_id: 9,
    });
    assert_eq!(state.chat(77).unwrap().unread_count, 4);

    state.reduce(&CoreEvent::ChatRead {
        chat_id: 77,
        max_message_id: 10,
    });
    assert_eq!(state.chat(77).unwrap().unread_count, 0);
}
