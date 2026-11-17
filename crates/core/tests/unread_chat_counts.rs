use tg_mobile_core::{ChatPage, ChatSummary, CoreEvent, CoreState};

fn chat(
    id: i64,
    unread_mentions_count: Option<u32>,
    unread_reactions_count: Option<u32>,
) -> ChatSummary {
    ChatSummary {
        id,
        title: format!("chat-{id}"),
        unread_count: 4,
        unread_mentions_count,
        unread_reactions_count,
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
        last_message: None,
    }
}

#[test]
fn refresh_without_counter_fields_preserves_known_telegram_values() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ChatsLoaded(vec![chat(7, Some(2), Some(3))]));
    state.reduce(&CoreEvent::ChatsLoaded(vec![chat(7, None, None)]));

    let summary = state.chat(7).unwrap();
    assert_eq!(summary.unread_mentions_count, Some(2));
    assert_eq!(summary.unread_reactions_count, Some(3));
}

#[test]
fn paginated_refresh_without_counter_fields_preserves_known_values() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ChatsLoaded(vec![chat(7, Some(2), Some(3))]));
    state.reduce(&CoreEvent::ChatsPageLoading { cursor: None });
    state.reduce(&CoreEvent::ChatsPageLoaded(ChatPage {
        requested_cursor: None,
        chats: vec![chat(7, None, None)],
        next_cursor: None,
    }));

    let summary = state.chat(7).unwrap();
    assert_eq!(summary.unread_mentions_count, Some(2));
    assert_eq!(summary.unread_reactions_count, Some(3));
}

#[test]
fn legacy_chat_json_deserializes_with_unknown_counters() {
    let chat: ChatSummary =
        serde_json::from_str(r#"{"id":7,"title":"legacy","unread_count":4,"last_message":null}"#)
            .unwrap();
    assert_eq!(chat.unread_mentions_count, None);
    assert_eq!(chat.unread_reactions_count, None);
}

#[test]
fn a_read_receipt_does_not_clear_server_reported_counters() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ChatsLoaded(vec![chat(7, Some(2), Some(3))]));
    state.reduce(&CoreEvent::ReadReceiptUpdated {
        chat_id: 7,
        direction: tg_mobile_core::ReadDirection::Inbox,
        max_message_id: 100,
    });

    let summary = state.chat(7).unwrap();
    assert_eq!(summary.unread_mentions_count, Some(2));
    assert_eq!(summary.unread_reactions_count, Some(3));
}
