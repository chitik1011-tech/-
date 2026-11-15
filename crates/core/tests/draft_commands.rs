use tg_mobile_core::{decode_command, CoreCommand, Draft};

fn valid_draft() -> Draft {
    Draft {
        chat_id: 7,
        text: "draft text".into(),
        reply_to_message_id: None,
        edit_message_id: None,
        updated_at_unix: 123,
    }
}

#[test]
fn update_draft_validates_chat_text_targets_and_timestamp() {
    let mut draft = valid_draft();
    assert!(CoreCommand::UpdateDraft(draft.clone()).validate().is_ok());

    draft.chat_id = 0;
    assert!(CoreCommand::UpdateDraft(draft.clone()).validate().is_err());

    draft = valid_draft();
    draft.text = " \t\n".into();
    assert!(CoreCommand::UpdateDraft(draft.clone()).validate().is_err());

    draft = valid_draft();
    draft.reply_to_message_id = Some(3);
    draft.edit_message_id = Some(4);
    assert!(CoreCommand::UpdateDraft(draft.clone()).validate().is_err());

    draft = valid_draft();
    draft.reply_to_message_id = Some(0);
    assert!(CoreCommand::UpdateDraft(draft).validate().is_err());

    draft = valid_draft();
    draft.edit_message_id = Some(-3);
    assert!(CoreCommand::UpdateDraft(draft).validate().is_err());

    draft = valid_draft();
    draft.updated_at_unix = -1;
    assert!(CoreCommand::UpdateDraft(draft).validate().is_err());
}

#[test]
fn clear_draft_validates_chat_and_expected_timestamp() {
    assert!(CoreCommand::ClearDraft {
        chat_id: 7,
        expected_updated_at_unix: Some(123),
        expected_draft: Some(valid_draft()),
    }
    .validate()
    .is_ok());
    assert!(CoreCommand::ClearDraft {
        chat_id: 0,
        expected_updated_at_unix: None,
        expected_draft: None,
    }
    .validate()
    .is_err());
    assert!(CoreCommand::ClearDraft {
        chat_id: 7,
        expected_updated_at_unix: Some(-1),
        expected_draft: Some(valid_draft()),
    }
    .validate()
    .is_err());
}

#[test]
fn draft_commands_round_trip_through_json_wire() {
    let commands = [
        CoreCommand::UpdateDraft(Draft {
            chat_id: 7,
            text: "reply in progress".into(),
            reply_to_message_id: Some(8),
            edit_message_id: None,
            updated_at_unix: 123,
        }),
        CoreCommand::ClearDraft {
            chat_id: 7,
            expected_updated_at_unix: Some(123),
            expected_draft: Some(Draft {
                chat_id: 7,
                text: "reply in progress".into(),
                reply_to_message_id: Some(8),
                edit_message_id: None,
                updated_at_unix: 123,
            }),
        },
    ];

    for command in commands {
        let json = serde_json::to_string(&command).unwrap();
        assert_eq!(decode_command(&json).unwrap(), command);
    }
}
