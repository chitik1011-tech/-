use tg_mobile_core::{ChatSummary, CoreEvent, CoreState, MessageSummary};

fn message(id: i64, timestamp_unix: i64) -> MessageSummary {
    MessageSummary {
        id,
        chat_id: 77,
        sender_id: 10,
        text: format!("history-{id}"),
        timestamp_unix,
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
fn loaded_history_merges_without_incrementing_unread() {
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        kind: None,
        id: 77,
        title: "history".into(),
        unread_count: 5,
        archived: Some(false),
        muted: Some(false),
        pinned: Some(false),
        is_unread: Some(true),
        last_message: None,
    });

    state.reduce(&CoreEvent::MessagesLoaded {
        chat_id: 77,
        messages: vec![message(2, 200), message(1, 100)],
    });

    assert_eq!(state.chat(77).unwrap().unread_count, 5);
    assert_eq!(
        state
            .messages_for_chat(77)
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(state.chat(77).unwrap().last_message.as_ref().unwrap().id, 2);
}

#[test]
fn history_ignores_messages_for_the_wrong_chat() {
    let mut state = CoreState::default();
    let wrong = MessageSummary {
        chat_id: 88,
        ..message(1, 100)
    };

    state.reduce(&CoreEvent::MessagesLoaded {
        chat_id: 77,
        messages: vec![wrong],
    });

    assert!(state.messages_for_chat(77).is_empty());
    assert!(state.messages_for_chat(88).is_empty());
}

#[test]
fn live_message_deduplicates_against_loaded_history() {
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        kind: None,
        id: 77,
        title: "history".into(),
        unread_count: 0,
        archived: Some(false),
        muted: Some(false),
        pinned: Some(false),
        is_unread: Some(false),
        last_message: None,
    });

    let historical = message(9, 900);
    state.reduce(&CoreEvent::MessagesLoaded {
        chat_id: 77,
        messages: vec![historical.clone()],
    });
    state.reduce(&CoreEvent::MessageAdded(historical));

    assert_eq!(state.messages_for_chat(77).len(), 1);
    assert_eq!(state.chat(77).unwrap().unread_count, 0);
}
