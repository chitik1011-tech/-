use tg_mobile_core::{encode_event, ChatSummary, CoreEvent, CoreState, MessageSummary};

fn message(chat_id: i64, id: i64, timestamp_unix: i64, text: &str) -> MessageSummary {
    MessageSummary {
        id,
        chat_id,
        sender_id: 42,
        text: text.into(),
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

fn seed_chat(state: &mut CoreState, chat_id: i64) {
    state.account.chats.push(ChatSummary {
        id: chat_id,
        title: format!("Known chat {chat_id}"),
        unread_count: 0,
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
        unread_mentions_count: None,
        unread_reactions_count: None,
        last_message: None,
    });
}

#[test]
fn edit_updates_cached_message_and_last_message_preview() {
    let mut state = CoreState::default();
    seed_chat(&mut state, 7);
    let original = message(7, 10, 100, "before");
    state.reduce(&CoreEvent::MessageAdded(original));

    let mut edited = message(7, 10, 100, "after");
    edited.edited = true;
    edited.edit_timestamp_unix = Some(150);
    state.reduce(&CoreEvent::MessageEdited(edited.clone()));

    assert_eq!(state.messages_for_chat(7), &[edited.clone()]);
    assert_eq!(
        state.chat(7).and_then(|chat| chat.last_message.as_ref()),
        Some(&edited)
    );
}

#[test]
fn stale_edit_cannot_roll_back_a_newer_cached_message_or_preview() {
    let mut state = CoreState::default();
    seed_chat(&mut state, 7);
    state.reduce(&CoreEvent::MessageAdded(message(7, 10, 100, "before")));

    let mut newest = message(7, 10, 100, "newest");
    newest.edited = true;
    newest.edit_timestamp_unix = Some(200);
    state.reduce(&CoreEvent::MessageEdited(newest.clone()));

    let mut stale = message(7, 10, 100, "stale");
    stale.edited = true;
    stale.edit_timestamp_unix = Some(150);
    state.reduce(&CoreEvent::MessageEdited(stale));

    assert_eq!(state.messages_for_chat(7), &[newest.clone()]);
    assert_eq!(
        state.chat(7).and_then(|chat| chat.last_message.as_ref()),
        Some(&newest)
    );
}

#[test]
fn delayed_history_page_cannot_roll_back_a_live_message_edit() {
    let mut state = CoreState::default();
    seed_chat(&mut state, 7);
    let original = message(7, 10, 100, "before");
    state.reduce(&CoreEvent::MessagesLoaded {
        chat_id: 7,
        messages: vec![original.clone()],
    });
    state.reduce(&CoreEvent::MessagesPageLoading {
        chat_id: 7,
        before_message_id: None,
    });

    let mut edited = original.clone();
    edited.text = "after".into();
    edited.edited = true;
    edited.edit_timestamp_unix = Some(150);
    state.reduce(&CoreEvent::MessageEdited(edited.clone()));

    // This page was requested before the live edit and therefore contains an
    // older Telegram snapshot for the same message ID.
    state.reduce(&CoreEvent::MessagesPageLoaded {
        chat_id: 7,
        before_message_id: None,
        messages: vec![original],
        next_before_message_id: None,
        has_more: false,
    });

    assert_eq!(state.messages_for_chat(7), &[edited.clone()]);
    assert_eq!(
        state.chat(7).and_then(|chat| chat.last_message.as_ref()),
        Some(&edited)
    );
}

#[test]
fn duplicate_edit_is_idempotent_and_does_not_clear_later_error() {
    let mut state = CoreState::default();
    seed_chat(&mut state, 7);
    state.reduce(&CoreEvent::MessageAdded(message(7, 10, 100, "before")));
    let mut edited = message(7, 10, 100, "after");
    edited.edited = true;
    edited.edit_timestamp_unix = Some(150);
    let event = CoreEvent::MessageEdited(edited.clone());
    state.reduce(&event);
    state.reduce(&CoreEvent::Error("keep this error".into()));

    state.reduce(&event);

    assert_eq!(state.messages_for_chat(7), &[edited.clone()]);
    assert_eq!(state.last_error.as_deref(), Some("keep this error"));
}

#[test]
fn edit_for_unknown_non_preview_message_does_not_create_sparse_history() {
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        id: 7,
        title: "chat".into(),
        unread_count: 0,
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
        unread_mentions_count: None,
        unread_reactions_count: None,
        last_message: Some(message(7, 20, 200, "latest")),
    });

    state.reduce(&CoreEvent::MessageEdited(message(7, 10, 100, "old edit")));

    assert!(state.messages_for_chat(7).is_empty());
    assert_eq!(state.chat(7).unwrap().last_message.as_ref().unwrap().id, 20);
}

#[test]
fn edit_updates_preview_even_when_history_page_is_not_cached() {
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        id: 7,
        title: "chat".into(),
        unread_count: 0,
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
        unread_mentions_count: None,
        unread_reactions_count: None,
        last_message: Some(message(7, 20, 200, "before")),
    });

    let mut edited = message(7, 20, 200, "after");
    edited.edited = true;
    state.reduce(&CoreEvent::MessageEdited(edited.clone()));

    assert!(state.messages_for_chat(7).is_empty());
    assert_eq!(
        state.chat(7).and_then(|chat| chat.last_message.as_ref()),
        Some(&edited)
    );
}

#[test]
fn scoped_delete_repairs_last_message_from_remaining_cache() {
    let mut state = CoreState::default();
    seed_chat(&mut state, 7);
    state.reduce(&CoreEvent::MessageAdded(message(7, 10, 100, "older")));
    state.reduce(&CoreEvent::MessageAdded(message(7, 11, 200, "latest")));

    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: Some(7),
        message_ids: vec![11],
    });

    assert_eq!(
        state
            .messages_for_chat(7)
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![10]
    );
    assert_eq!(
        state
            .chat(7)
            .and_then(|chat| chat.last_message.as_ref())
            .map(|m| m.id),
        Some(10)
    );
}

#[test]
fn deleting_ordered_preview_falls_back_after_out_of_order_message_add() {
    let mut state = CoreState::default();
    seed_chat(&mut state, 7);
    state.reduce(&CoreEvent::MessageAdded(message(7, 20, 200, "newest")));
    state.reduce(&CoreEvent::MessageAdded(message(
        7,
        10,
        100,
        "older update",
    )));

    assert_eq!(
        state
            .chat(7)
            .and_then(|chat| chat.last_message.as_ref())
            .map(|m| m.id),
        Some(20)
    );

    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: Some(7),
        message_ids: vec![20],
    });

    assert_eq!(
        state
            .messages_for_chat(7)
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![10]
    );
    assert_eq!(
        state
            .chat(7)
            .and_then(|chat| chat.last_message.as_ref())
            .map(|m| m.id),
        Some(10)
    );
}

#[test]
fn paginated_cache_keeps_cursor_through_edit_and_preview_delete_fallback() {
    let mut state = CoreState::default();
    seed_chat(&mut state, 7);
    let newest = message(7, 20, 200, "newest");
    let older = message(7, 10, 100, "older");
    state.reduce(&CoreEvent::MessageAdded(newest.clone()));
    state.reduce(&CoreEvent::MessagesPageLoading {
        chat_id: 7,
        before_message_id: None,
    });
    state.reduce(&CoreEvent::MessagesPageLoaded {
        chat_id: 7,
        before_message_id: None,
        messages: vec![older.clone(), newest],
        next_before_message_id: Some(9),
        has_more: true,
    });

    let mut edited_older = older;
    edited_older.text = "older edited".into();
    edited_older.edited = true;
    edited_older.edit_timestamp_unix = Some(150);
    state.reduce(&CoreEvent::MessageEdited(edited_older.clone()));
    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: Some(7),
        message_ids: vec![20],
    });

    assert_eq!(state.messages_for_chat(7), &[edited_older.clone()]);
    assert_eq!(
        state.chat(7).and_then(|chat| chat.last_message.as_ref()),
        Some(&edited_older)
    );
    let history = state.message_history_for_chat(7).unwrap();
    assert!(!history.loading);
    assert!(history.has_more);
    assert_eq!(history.next_before_message_id, Some(9));
}

#[test]
fn global_delete_batch_applies_across_cached_non_channel_histories() {
    let mut state = CoreState::default();
    seed_chat(&mut state, 1);
    seed_chat(&mut state, 2);
    state.reduce(&CoreEvent::MessageAdded(message(1, 100, 100, "a")));
    state.reduce(&CoreEvent::MessageAdded(message(2, 200, 100, "b")));
    state.reduce(&CoreEvent::MessageAdded(message(2, 201, 200, "c")));

    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: None,
        message_ids: vec![100, 200, 0, -1],
    });

    assert!(state.messages_for_chat(1).is_empty());
    assert_eq!(
        state
            .messages_for_chat(2)
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![201]
    );
    assert!(state.chat(1).unwrap().last_message.is_none());
    assert_eq!(
        state
            .chat(2)
            .unwrap()
            .last_message
            .as_ref()
            .map(|message| message.id),
        Some(201)
    );
}

#[test]
fn deleting_preview_without_cached_history_clears_stale_preview() {
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        id: 7,
        title: "chat".into(),
        unread_count: 3,
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
        unread_mentions_count: None,
        unread_reactions_count: None,
        last_message: Some(message(7, 50, 500, "preview only")),
    });

    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: Some(7),
        message_ids: vec![50],
    });

    assert!(state.chat(7).unwrap().last_message.is_none());
    assert_eq!(state.chat(7).unwrap().unread_count, 3);
}

#[test]
fn delete_batch_deduplicates_ids_and_ignores_invalid_ids() {
    let mut state = CoreState::default();
    seed_chat(&mut state, 7);
    state.reduce(&CoreEvent::MessageAdded(message(7, 10, 100, "one")));
    state.reduce(&CoreEvent::MessageAdded(message(7, 11, 200, "two")));

    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: Some(7),
        message_ids: vec![10, 10, 0, -9],
    });

    assert_eq!(
        state
            .messages_for_chat(7)
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![11]
    );
}

#[test]
fn duplicate_delete_is_idempotent_and_does_not_clear_later_error() {
    let mut state = CoreState::default();
    seed_chat(&mut state, 7);
    state.reduce(&CoreEvent::MessageAdded(message(7, 10, 100, "one")));
    let event = CoreEvent::MessagesDeleted {
        chat_id: Some(7),
        message_ids: vec![10],
    };
    state.reduce(&event);
    state.reduce(&CoreEvent::Error("keep this error".into()));

    state.reduce(&event);

    assert!(state.messages_for_chat(7).is_empty());
    assert!(state.chat(7).unwrap().last_message.is_none());
    assert_eq!(state.last_error.as_deref(), Some("keep this error"));
}

#[test]
fn mutation_events_have_stable_wire_names() {
    let edited = CoreEvent::MessageEdited(message(7, 10, 100, "edited"));
    assert_eq!(edited.kind(), "message_edited");
    let edited_json = encode_event(&edited).unwrap();
    assert!(edited_json.contains("\"type\":\"message_edited\""));

    let deleted = CoreEvent::MessagesDeleted {
        chat_id: Some(7),
        message_ids: vec![10, 11],
    };
    assert_eq!(deleted.kind(), "messages_deleted");
    let deleted_json = encode_event(&deleted).unwrap();
    assert!(deleted_json.contains("\"type\":\"messages_deleted\""));
}
