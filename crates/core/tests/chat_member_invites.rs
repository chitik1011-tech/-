use tg_mobile_core::{decode_command, encode_event, CoreCommand, CoreError, CoreEvent};

#[test]
fn invite_user_command_validates_ids_and_round_trips_on_wire() {
    let command = CoreCommand::InviteUserToChat {
        chat_id: -42,
        user_id: 1234,
    };
    assert_eq!(command.kind(), "invite_user_to_chat");
    assert!(command.validate().is_ok());

    let encoded = serde_json::to_string(&command).unwrap();
    assert_eq!(decode_command(&encoded).unwrap(), command);
}

#[test]
fn invite_user_command_rejects_zero_peer_ids() {
    for command in [
        CoreCommand::InviteUserToChat {
            chat_id: 0,
            user_id: 1234,
        },
        CoreCommand::InviteUserToChat {
            chat_id: -42,
            user_id: 0,
        },
        CoreCommand::InviteUserToChat {
            chat_id: -42,
            user_id: -1234,
        },
    ] {
        assert!(matches!(
            command.validate(),
            Err(CoreError::InvalidInput(_))
        ));
    }
}

#[test]
fn invite_success_event_round_trips_as_a_typed_result() {
    let event = CoreEvent::ChatMemberInvited {
        chat_id: -42,
        user_id: 1234,
    };
    let encoded = encode_event(&event).unwrap();
    let decoded: CoreEvent = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, event);
    assert_eq!(decoded.kind(), "chat_member_invited");
}
