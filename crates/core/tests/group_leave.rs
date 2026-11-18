use tg_mobile_core::{
    ChatCapabilities, ChatKind, ChatSummary, CoreCommand, CoreEvent, CoreState, decode_command,
};

#[test]
fn leave_chat_command_has_a_stable_wire_kind_and_rejects_zero_id() {
    let command = CoreCommand::LeaveChat { chat_id: -42 };
    assert_eq!(command.kind(), "leave_chat");
    assert!(command.validate().is_ok());

    let encoded = serde_json::to_string(&command).unwrap();
    assert_eq!(decode_command(&encoded).unwrap(), command);
    assert!(CoreCommand::LeaveChat { chat_id: 0 }.validate().is_err());
}

#[test]
fn chat_left_removes_group_scoped_snapshots_and_round_trips() {
    let chat_id = -42;
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        id: chat_id,
        title: "Example group".into(),
        unread_count: 2,
        unread_mentions_count: None,
        unread_reactions_count: None,
        archived: Some(false),
        muted: Some(false),
        pinned: Some(false),
        is_unread: Some(true),
        last_message: None,
    });
    state.chat_capabilities.insert(
        chat_id,
        ChatCapabilities {
            chat_id,
            chat_kind: ChatKind::Group,
            ..ChatCapabilities::default()
        },
    );

    let event = CoreEvent::ChatLeft { chat_id };
    let encoded = serde_json::to_string(&event).unwrap();
    let decoded: CoreEvent = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.kind(), "chat_left");
    state.reduce(&decoded);

    assert!(state.chat(chat_id).is_none());
    assert!(!state.chat_capabilities.contains_key(&chat_id));
}
