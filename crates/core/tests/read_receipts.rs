use tg_mobile_core::{
    decode_command, ChatSummary, CoreCommand, CoreError, CoreEvent, CoreState, MessageSummary,
    ReadDirection,
};

fn last_message(id: i64) -> MessageSummary {
    MessageSummary {
        id,
        chat_id: 77,
        sender_id: 12,
        text: format!("message-{id}"),
        timestamp_unix: id,
        outgoing: false,
        location: None,
        attachments: Vec::new(),
        forwarded_from: None,
        edited: false,
        edit_timestamp_unix: None,
        service_message: false,
        service: None,
        media_caption: None,
        reply_to: None,

        reactions: Vec::new(),
        reaction_revision: None,
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
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
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

#[test]
fn inbox_and_outbox_receipts_are_monotonic_and_persisted() {
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        id: 77,
        title: "chat".into(),
        unread_count: 3,
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
        last_message: Some(last_message(25)),
    });
    for (direction, max_message_id) in [
        (ReadDirection::Outbox, 18),
        (ReadDirection::Inbox, 25),
        (ReadDirection::Outbox, 12),
        (ReadDirection::Outbox, 18),
    ] {
        state.reduce(&CoreEvent::ReadReceiptUpdated {
            chat_id: 77,
            direction,
            max_message_id,
        });
    }

    let receipt = state.read_receipts.get(&77).unwrap();
    assert_eq!(receipt.inbox_max_message_id, 25);
    assert_eq!(receipt.outbox_max_message_id, 18);
    assert_eq!(state.chat(77).unwrap().unread_count, 0);

    let serialized = serde_json::to_string(&state).unwrap();
    let restored: CoreState = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored.read_receipts, state.read_receipts);
}

#[test]
fn read_receipt_wire_event_round_trips() {
    let event = CoreEvent::ReadReceiptUpdated {
        chat_id: 77,
        direction: ReadDirection::Inbox,
        max_message_id: 42,
    };
    let json = serde_json::to_string(&event).unwrap();
    let decoded: CoreEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, event);
}

#[test]
fn stale_connection_receipts_are_ignored_after_reconnect() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ConnectionSessionStarted);
    let old_token = state.connection_machine.token();
    for event in [
        tg_mobile_core::LinkEvent::TransportConnected,
        tg_mobile_core::LinkEvent::SyncStarted,
        tg_mobile_core::LinkEvent::SyncCompleted,
        tg_mobile_core::LinkEvent::TransportLost,
    ] {
        state.reduce(&CoreEvent::ConnectionLifecycle {
            token: old_token,
            event,
        });
    }
    let new_token = state.connection_machine.token();
    assert_ne!(old_token, new_token);

    state.reduce(&CoreEvent::SessionUpdate {
        token: old_token,
        event: Box::new(CoreEvent::ReadReceiptUpdated {
            chat_id: 77,
            direction: ReadDirection::Outbox,
            max_message_id: 42,
        }),
    });
    assert!(!state.read_receipts.contains_key(&77));

    state.reduce(&CoreEvent::SessionUpdate {
        token: new_token,
        event: Box::new(CoreEvent::ReadReceiptUpdated {
            chat_id: 77,
            direction: ReadDirection::Outbox,
            max_message_id: 42,
        }),
    });
    state.reduce(&CoreEvent::SessionUpdate {
        token: new_token,
        event: Box::new(CoreEvent::ReadReceiptUpdated {
            chat_id: 77,
            direction: ReadDirection::Outbox,
            max_message_id: 40,
        }),
    });
    assert_eq!(state.read_receipts[&77].outbox_max_message_id, 42);
}
