use tg_mobile_core::{ChatPage, ChatSummary, CoreCommand, CoreEvent, CoreState, MessageSummary};

fn summary(id: i64, title: &str, unread_count: u32, last_message: Option<MessageSummary>) -> ChatSummary {
    ChatSummary {
        id,
        title: title.into(),
        unread_count,
        last_message,
    }
}

fn message(id: i64, chat_id: i64, timestamp_unix: i64, text: &str) -> MessageSummary {
    MessageSummary {
        id,
        chat_id,
        sender_id: 42,
        text: text.into(),
        timestamp_unix,
        outgoing: false,
    }
}

fn page(
    requested_cursor: Option<&str>,
    chats: Vec<ChatSummary>,
    next_cursor: Option<&str>,
) -> CoreEvent {
    CoreEvent::ChatsPageLoaded(ChatPage {
        requested_cursor: requested_cursor.map(str::to_owned),
        chats,
        next_cursor: next_cursor.map(str::to_owned),
    })
}

#[test]
fn zero_chat_page_marks_pagination_complete() {
    let mut state = CoreState::default();

    state.reduce(&page(None, Vec::new(), None));

    assert!(state.account.chats.is_empty());
    assert!(state.chat_pagination.loaded);
    assert_eq!(state.chat_pagination.next_cursor, None);
}

#[test]
fn one_page_exposes_no_next_cursor() {
    let mut state = CoreState::default();

    state.reduce(&page(
        None,
        vec![summary(1, "one", 0, None)],
        None,
    ));

    assert_eq!(state.account.chats.len(), 1);
    assert_eq!(state.account.chats[0].id, 1);
    assert_eq!(state.chat_pagination.next_cursor, None);
}

#[test]
fn multiple_pages_merge_duplicates_without_reordering() {
    let mut state = CoreState::default();
    let old_message = message(1, 2, 100, "old");
    let new_message = message(2, 2, 200, "new");

    state.reduce(&page(
        None,
        vec![
            summary(1, "first", 0, None),
            summary(2, "second", 1, Some(old_message)),
        ],
        Some("cursor-2"),
    ));

    assert_eq!(
        state.chat_pagination.next_cursor.as_deref(),
        Some("cursor-2")
    );

    state.reduce(&page(
        Some("cursor-2"),
        vec![
            summary(2, "renamed", 2, Some(new_message.clone())),
            summary(3, "third", 0, None),
            summary(3, "third updated", 1, None),
        ],
        None,
    ));

    assert_eq!(
        state.account.chats.iter().map(|chat| chat.id).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(state.account.chats[1].title, "renamed");
    assert_eq!(state.account.chats[1].unread_count, 2);
    assert_eq!(
        state.account.chats[1].last_message.as_ref(),
        Some(&new_message)
    );
    assert_eq!(state.account.chats[2].unread_count, 1);
    assert_eq!(state.chat_pagination.next_cursor, None);
    assert!(state.chat_pagination.consumed_cursors.contains("cursor-2"));
}

#[test]
fn live_messages_survive_next_page_and_stale_page_summaries() {
    let mut state = CoreState::default();
    let first_message = message(1, 10, 100, "first");
    let live_message = message(2, 10, 200, "live");
    let new_chat_message = message(3, 30, 150, "new chat");

    state.reduce(&page(
        None,
        vec![summary(10, "ten", 1, Some(first_message))],
        Some("cursor-2"),
    ));
    state.reduce(&CoreEvent::MessageAdded(live_message.clone()));
    state.reduce(&CoreEvent::MessageAdded(new_chat_message.clone()));
    state.reduce(&page(
        Some("cursor-2"),
        vec![
            summary(10, "ten", 1, Some(message(1, 10, 100, "stale"))),
            summary(20, "twenty", 0, None),
            summary(30, "thirty", 0, Some(message(2, 30, 140, "stale chat data"))),
        ],
        None,
    ));

    assert_eq!(
        state.account.chats.iter().map(|chat| chat.id).collect::<Vec<_>>(),
        vec![10, 30, 20]
    );
    let live_chat = state.chat(10).expect("live chat remains present");
    assert_eq!(live_chat.unread_count, 2);
    assert_eq!(live_chat.last_message.as_ref(), Some(&live_message));
    let new_chat = state.chat(30).expect("live-created chat remains present");
    assert_eq!(new_chat.title, "thirty");
    assert_eq!(new_chat.last_message.as_ref(), Some(&new_chat_message));
    assert_eq!(state.messages_for_chat(10), &[live_message]);
    assert_eq!(state.messages_for_chat(30), &[new_chat_message]);
    assert_eq!(state.chat_pagination.next_cursor, None);
}

#[test]
fn live_message_received_before_first_page_is_preserved() {
    let mut state = CoreState::default();
    let live_message = message(9, 90, 900, "arrived early");

    state.reduce(&CoreEvent::MessageAdded(live_message.clone()));
    state.reduce(&page(
        None,
        vec![summary(1, "server chat", 0, None)],
        None,
    ));

    assert_eq!(
        state.account.chats.iter().map(|chat| chat.id).collect::<Vec<_>>(),
        vec![1, 90]
    );
    assert_eq!(state.chat(90).unwrap().last_message.as_ref(), Some(&live_message));
}

#[test]
fn stale_or_repeated_page_cursor_is_ignored_and_loops_stop() {
    let mut state = CoreState::default();
    state.reduce(&page(None, vec![summary(1, "one", 0, None)], Some("cursor-2")));

    state.reduce(&page(
        Some("wrong-cursor"),
        vec![summary(2, "stale", 0, None)],
        None,
    ));
    assert_eq!(state.account.chats.iter().map(|chat| chat.id).collect::<Vec<_>>(), vec![1]);
    assert_eq!(
        state.chat_pagination.next_cursor.as_deref(),
        Some("cursor-2")
    );

    state.reduce(&page(
        Some("cursor-2"),
        vec![summary(2, "two", 0, None)],
        Some("cursor-2"),
    ));
    assert_eq!(state.chat_pagination.next_cursor, None);
}

#[test]
fn page_command_validates_limit_and_nonblank_cursor() {
    assert!(CoreCommand::LoadChatsPage {
        cursor: None,
        limit: 50,
    }
    .validate()
    .is_ok());
    assert!(CoreCommand::LoadChatsPage {
        cursor: Some("opaque-token".into()),
        limit: 50,
    }
    .validate()
    .is_ok());
    assert!(CoreCommand::LoadChatsPage {
        cursor: Some("  ".into()),
        limit: 50,
    }
    .validate()
    .is_err());
    assert!(CoreCommand::LoadChatsPage {
        cursor: None,
        limit: 0,
    }
    .validate()
    .is_err());
}
