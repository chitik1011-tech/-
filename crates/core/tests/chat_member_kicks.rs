use tg_mobile_core::{
    decode_command, encode_event, ChatMemberSummary, CoreCommand, CoreError, CoreEvent, CoreState,
    PeerProfile,
};

#[test]
fn kick_command_validates_ids_and_round_trips_on_wire() {
    let command = CoreCommand::KickChatMember {
        chat_id: -42,
        user_id: 1234,
    };
    assert_eq!(command.kind(), "kick_chat_member");
    assert!(command.validate().is_ok());
    assert_eq!(
        decode_command(&serde_json::to_string(&command).unwrap()).unwrap(),
        command
    );

    for command in [
        CoreCommand::KickChatMember {
            chat_id: 0,
            user_id: 1234,
        },
        CoreCommand::KickChatMember {
            chat_id: -42,
            user_id: 0,
        },
        CoreCommand::KickChatMember {
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
fn successful_kick_updates_cached_member_snapshot() {
    let mut state = CoreState::default();
    let mut profile = PeerProfile::unknown(-42);
    profile.member_count = Some(3);
    profile.members = Some(vec![
        ChatMemberSummary {
            user_id: 1,
            display_name: "Owner".into(),
            username: None,
            role: "owner".into(),
        },
        ChatMemberSummary {
            user_id: 2,
            display_name: "Removed member".into(),
            username: None,
            role: "member".into(),
        },
        ChatMemberSummary {
            user_id: 3,
            display_name: "Other member".into(),
            username: None,
            role: "member".into(),
        },
    ]);
    state.reduce(&CoreEvent::ProfileUpdated(profile));

    let event = CoreEvent::ChatMemberKicked {
        chat_id: -42,
        user_id: 2,
    };
    assert!(encode_event(&event).unwrap().contains("chat_member_kicked"));
    state.reduce(&event);

    let profile = state.profiles.get(&-42).unwrap();
    assert_eq!(profile.member_count, Some(2));
    assert!(profile
        .members
        .as_ref()
        .unwrap()
        .iter()
        .all(|member| member.user_id != 2));
}
