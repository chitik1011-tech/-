use tg_mobile_core::{decode_command, CoreCommand};

#[test]
fn bulk_delete_command_round_trips_and_rejects_invalid_selections() {
    let command = CoreCommand::DeleteMessages {
        chat_id: 42,
        message_ids: vec![8, 3, 5],
    };
    assert!(command.validate().is_ok());
    let encoded = serde_json::to_string(&command).unwrap();
    assert_eq!(decode_command(&encoded).unwrap(), command);

    for message_ids in [
        Vec::new(),
        vec![0],
        vec![-1],
        vec![1, 1],
        (1..=101).collect(),
    ] {
        assert!(CoreCommand::DeleteMessages {
            chat_id: 42,
            message_ids,
        }
        .validate()
        .is_err());
    }
    assert!(CoreCommand::DeleteMessages {
        chat_id: 0,
        message_ids: vec![1],
    }
    .validate()
    .is_err());
}
