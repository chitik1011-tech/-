use tg_mobile_core::{decode_command, ChatPage, ChatSummary, CoreCommand, CoreEvent, CoreState};

fn chat(id: i64, title: &str, message_id: i64) -> ChatSummary {
    ChatSummary {
        id,
        title: title.into(),
        unread_count: 0,
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
        last_message: (message_id > 0).then(|| tg_mobile_core::MessageSummary {
            id: message_id,
            chat_id: id,
            sender_id: id,
            text: format!("message {message_id}"),
            timestamp_unix: message_id,
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
        }),
    }
}

#[test]
fn page_command_validates_cursor_and_round_trips_on_wire() {
    let command = CoreCommand::LoadChatsPage {
        cursor: Some("100".into()),
        limit: 50,
    };
    assert_eq!(command.kind(), "load_chats_page");
    assert!(command.validate().is_ok());
    assert!(CoreCommand::LoadChatsPage {
        cursor: Some("  ".into()),
        limit: 50,
    }
    .validate()
    .is_err());
    assert!(CoreCommand::LoadChatsPage {
        cursor: None,
        limit: 101,
    }
    .validate()
    .is_err());
    let encoded = serde_json::to_string(&command).unwrap();
    assert_eq!(decode_command(&encoded).unwrap(), command);
}

#[test]
fn pages_merge_in_server_order_deduplicate_and_keep_newer_live_messages() {
    let mut state = CoreState::default();
    state.account.chats = vec![chat(1, "one", 20), chat(2, "two", 10)];
    state.reduce(&CoreEvent::ChatsPageLoading { cursor: None });
    state.reduce(&CoreEvent::ChatsPageLoaded(ChatPage {
        requested_cursor: None,
        chats: vec![chat(1, "one", 5), chat(3, "three", 0)],
        next_cursor: Some("2".into()),
    }));
    assert_eq!(
        state
            .account
            .chats
            .iter()
            .map(|chat| chat.id)
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(state.account.chats[0].last_message.as_ref().unwrap().id, 20);

    state.reduce(&CoreEvent::ChatsPageLoading {
        cursor: Some("2".into()),
    });
    state.reduce(&CoreEvent::ChatsPageLoaded(ChatPage {
        requested_cursor: Some("2".into()),
        chats: vec![chat(3, "three refreshed", 0), chat(4, "four", 0)],
        next_cursor: None,
    }));
    assert_eq!(
        state
            .account
            .chats
            .iter()
            .map(|chat| chat.id)
            .collect::<Vec<_>>(),
        vec![1, 3, 4]
    );
    assert_eq!(state.account.chats[1].title, "three refreshed");
    assert!(!state.chat_pagination.can_load_next());
}

#[test]
fn stale_completions_are_ignored_and_failed_cursor_can_be_retried() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ChatsPageLoading {
        cursor: Some("10".into()),
    });
    state.reduce(&CoreEvent::ChatsPageFailed {
        cursor: Some("old".into()),
        error: "stale".into(),
    });
    assert!(state.chat_pagination.loading);
    state.reduce(&CoreEvent::ChatsPageFailed {
        cursor: Some("10".into()),
        error: "network down".into(),
    });
    assert!(!state.chat_pagination.loading);
    assert_eq!(state.chat_pagination.next_cursor.as_deref(), Some("10"));
    assert!(state.chat_pagination.can_load_next());
    state.reduce(&CoreEvent::ChatsPageLoading {
        cursor: Some("10".into()),
    });
    state.reduce(&CoreEvent::ChatsPageLoaded(ChatPage {
        requested_cursor: Some("wrong".into()),
        chats: vec![chat(11, "stale", 0)],
        next_cursor: None,
    }));
    assert!(state.chat_pagination.loading);
    assert!(state.account.chats.is_empty());
}

#[test]
fn repeated_cursor_stops_pagination_and_old_state_defaults_cursor_bookkeeping() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ChatsPageLoading { cursor: None });
    state.reduce(&CoreEvent::ChatsPageLoaded(ChatPage {
        requested_cursor: None,
        chats: vec![chat(1, "one", 0)],
        next_cursor: Some("again".into()),
    }));
    state.reduce(&CoreEvent::ChatsPageLoading {
        cursor: Some("again".into()),
    });
    state.reduce(&CoreEvent::ChatsPageLoaded(ChatPage {
        requested_cursor: Some("again".into()),
        chats: vec![chat(2, "two", 0)],
        next_cursor: Some("again".into()),
    }));
    assert!(!state.chat_pagination.can_load_next());
    assert!(state.chat_pagination.error.is_some());

    let legacy = r#"{"running":true,"account":{"authorization":"Closed","chats":[]},"messages":{},"last_error":null}"#;
    let restored: CoreState = serde_json::from_str(legacy).unwrap();
    assert!(!restored.chat_pagination.loaded);
    assert!(restored.chat_pagination.can_load_next());
}
