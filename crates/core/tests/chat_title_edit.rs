use tg_mobile_core::{
    decode_command, encode_event, CoreCommand, CoreError, CoreEvent, CoreState, PeerProfile,
    MAX_CHAT_TITLE_CHARS,
};

#[test]
fn edit_chat_title_validates_and_round_trips_on_wire() {
    let command = CoreCommand::EditChatTitle {
        chat_id: -42,
        title: "  Project Orion  ".into(),
    };
    assert_eq!(command.kind(), "edit_chat_title");
    assert!(command.validate().is_ok());
    assert_eq!(
        decode_command(&serde_json::to_string(&command).unwrap()).unwrap(),
        command
    );

    assert!(CoreCommand::EditChatTitle {
        chat_id: 0,
        title: "Valid".into(),
    }
    .validate()
    .is_err());
    assert!(matches!(
        CoreCommand::EditChatTitle {
            chat_id: -42,
            title: "   ".into(),
        }
        .validate(),
        Err(CoreError::InvalidInput(_))
    ));
    assert!(CoreCommand::EditChatTitle {
        chat_id: -42,
        title: "💬".repeat(MAX_CHAT_TITLE_CHARS + 1),
    }
    .validate()
    .is_err());
}

#[test]
fn title_changed_event_updates_cached_dialog_and_profile() {
    let mut state = CoreState::default();
    state.account.chats.push(tg_mobile_core::ChatSummary {
        id: -42,
        title: "Old title".into(),
        unread_count: 0,
        unread_mentions_count: None,
        unread_reactions_count: None,
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
        last_message: None,
    });
    let mut profile = PeerProfile::unknown(-42);
    profile.display_name = Some("Old title".into());
    state.reduce(&CoreEvent::ProfileUpdated(profile));

    let event = CoreEvent::ChatTitleChanged {
        chat_id: -42,
        title: "New title".into(),
    };
    assert!(encode_event(&event).unwrap().contains("chat_title_changed"));
    state.reduce(&event);

    assert_eq!(state.account.chats[0].title, "New title");
    assert_eq!(
        state.profiles.get(&-42).unwrap().display_name.as_deref(),
        Some("New title")
    );
}
