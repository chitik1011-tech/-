use tg_mobile_core::{
    CoreCommand, CoreEvent, CoreState, MessageMutationAction, MAX_EDIT_MESSAGE_TEXT_BYTES,
};

#[test]
fn edit_and_delete_for_everyone_commands_validate_and_round_trip() {
    let edit = CoreCommand::EditMessage {
        chat_id: -100123,
        message_id: 42,
        text: "updated text".into(),
    };
    assert!(edit.validate().is_ok());
    assert_eq!(
        serde_json::from_str::<CoreCommand>(&serde_json::to_string(&edit).unwrap()).unwrap(),
        edit,
    );

    let delete = CoreCommand::DeleteMessageForEveryone {
        chat_id: -100123,
        message_id: 42,
    };
    assert!(delete.validate().is_ok());
    assert_eq!(delete.kind(), "delete_message_for_everyone");
    assert_eq!(
        serde_json::from_str::<CoreCommand>(&serde_json::to_string(&delete).unwrap()).unwrap(),
        delete,
    );
}

#[test]
fn edit_validation_rejects_invalid_targets_empty_text_and_oversized_text() {
    for (chat_id, message_id) in [(0, 1), (1, 0), (1, -1)] {
        assert!(CoreCommand::EditMessage {
            chat_id,
            message_id,
            text: "text".into(),
        }
        .validate()
        .is_err());
        assert!(CoreCommand::DeleteMessageForEveryone { chat_id, message_id }
            .validate()
            .is_err());
    }

    assert!(CoreCommand::EditMessage {
        chat_id: 1,
        message_id: 1,
        text: "  ".into(),
    }
    .validate()
    .is_err());
    assert!(CoreCommand::EditMessage {
        chat_id: 1,
        message_id: 1,
        text: "x".repeat(MAX_EDIT_MESSAGE_TEXT_BYTES + 1),
    }
    .validate()
    .is_err());
}

#[test]
fn mutation_failure_is_ephemeral_and_does_not_change_server_message_cache() {
    let mut state = CoreState::default();
    let before = state.messages.clone();
    state.reduce(&CoreEvent::MessageMutationFailed {
        chat_id: 1,
        message_id: 9,
        action: MessageMutationAction::DeleteForEveryone,
        error: "permission denied".into(),
    });
    assert_eq!(state.messages, before);
    assert_eq!(state.last_error, None);
}
