use tg_mobile_core::{
    ChatSummary, ChatUnreadState, ChatUnreadUpdate, CoreEvent, CoreState, MessageSummary,
};

fn chat(id: i64, unread_count: u32) -> ChatSummary {
    ChatSummary {
        id,
        title: format!("chat-{id}"),
        unread_count,
        last_message: None,
    }
}

fn message(id: i64, chat_id: i64, outgoing: bool) -> MessageSummary {
    MessageSummary {
        id,
        chat_id,
        sender_id: 42,
        text: format!("message-{id}"),
        timestamp_unix: id,
        outgoing,
    }
}

#[test]
fn partial_updates_are_idempotent_and_preserve_unreported_counters() {
    let mut state = CoreState::default();
    state.account.chats.push(chat(10, 6));
    let update = CoreUnreadUpdate::complete(10);

    state.reduce(&update);
    state.reduce(&update);

    assert_eq!(
        state.unread_state_for_chat(10),
        Some(ChatUnreadState {
            unread_messages: 6,
            unread_mentions: 2,
            unread_reactions: 3,
            muted: true,
            unread_mark: true,
        })
    );
    let totals = state.unread_totals();
    assert_eq!(totals.unread_messages, 6);
    assert_eq!(totals.unread_mentions, 2);
    assert_eq!(totals.unread_reactions, 3);
    assert_eq!(totals.unread_chats, 1);
    assert_eq!(totals.muted_chats, 1);
    assert_eq!(totals.marked_unread_chats, 1);
    assert_eq!(totals.badge_count, 6);
}

#[test]
fn read_receipt_message_count_does_not_clear_mentions_or_reactions() {
    let mut state = CoreState::default();
    state.account.chats.push(chat(10, 6));
    state.reduce(&CoreUnreadUpdate::complete(10));

    state.reduce(&CoreEvent::ChatUnreadUpdated(ChatUnreadUpdate {
        chat_id: 10,
        unread_messages: Some(0),
        ..ChatUnreadUpdate::default()
    }));

    let unread = state.unread_state_for_chat(10).unwrap();
    assert_eq!(unread.unread_messages, 0);
    assert_eq!(unread.unread_mentions, 2);
    assert_eq!(unread.unread_reactions, 3);
    assert!(unread.muted);
    assert!(unread.unread_mark);
    assert_eq!(state.chat(10).unwrap().unread_count, 0);
}

#[test]
fn duplicate_message_update_increments_only_once_and_keeps_other_counters() {
    let mut state = CoreState::default();
    state.account.chats.push(chat(10, 4));
    state.reduce(&CoreEvent::ChatUnreadUpdated(ChatUnreadUpdate {
        chat_id: 10,
        unread_mentions: Some(1),
        unread_reactions: Some(2),
        ..ChatUnreadUpdate::default()
    }));

    let incoming = message(8, 10, false);
    state.reduce(&CoreEvent::MessageAdded(incoming.clone()));
    state.reduce(&CoreEvent::MessageAdded(incoming));

    let unread = state.unread_state_for_chat(10).unwrap();
    assert_eq!(unread.unread_messages, 5);
    assert_eq!(unread.unread_mentions, 1);
    assert_eq!(unread.unread_reactions, 2);
}

#[test]
fn chat_list_refresh_updates_message_count_but_preserves_other_unread_fields() {
    let mut state = CoreState::default();
    state.reduce(&CoreUnreadUpdate::complete(10));
    state.reduce(&CoreEvent::ChatsLoaded(vec![chat(10, 9)]));

    let unread = state.unread_state_for_chat(10).unwrap();
    assert_eq!(unread.unread_messages, 9);
    assert_eq!(unread.unread_mentions, 2);
    assert_eq!(unread.unread_reactions, 3);
}

#[test]
fn unread_aggregate_includes_unlisted_peer_updates_without_double_counting() {
    let mut state = CoreState::default();
    state.account.chats = vec![chat(10, 2), chat(11, 4)];
    state.reduce(&CoreEvent::ChatUnreadUpdated(ChatUnreadUpdate {
        chat_id: 99,
        unread_messages: Some(3),
        unread_mentions: Some(1),
        ..ChatUnreadUpdate::default()
    }));

    let totals = state.unread_totals();
    assert_eq!(totals.unread_messages, 9);
    assert_eq!(totals.unread_mentions, 1);
    assert_eq!(totals.badge_count, 9);
}

#[test]
fn old_persisted_state_without_unread_map_keeps_legacy_message_counts() {
    let legacy = r#"{
        "running": true,
        "account": {
            "authorization": "Ready",
            "chats": [{
                "id": 10,
                "title": "legacy",
                "unread_count": 7,
                "last_message": null
            }]
        },
        "messages": {},
        "last_error": null
    }"#;

    let state: CoreState = serde_json::from_str(legacy).unwrap();
    assert!(state.unread_by_chat.is_empty());
    assert_eq!(state.unread_state_for_chat(10).unwrap().unread_messages, 7);
    assert_eq!(state.total_unread_count(), 7);
}

#[test]
fn unread_update_has_stable_wire_kind_and_round_trips() {
    let event = CoreUnreadUpdate::complete(10);
    assert_eq!(event.kind(), "chat_unread_updated");

    let encoded = serde_json::to_string(&event).unwrap();
    let decoded: CoreEvent = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, event);
}

#[test]
fn zero_chat_id_is_ignored_without_mutating_state() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ChatUnreadUpdated(ChatUnreadUpdate {
        chat_id: 0,
        unread_messages: Some(9),
        unread_mentions: Some(4),
        ..ChatUnreadUpdate::default()
    }));

    assert!(state.unread_by_chat.is_empty());
    assert!(state.account.chats.is_empty());
}

struct CoreUnreadUpdate;

impl CoreUnreadUpdate {
    fn complete(chat_id: i64) -> CoreEvent {
        CoreEvent::ChatUnreadUpdated(ChatUnreadUpdate {
            chat_id,
            unread_messages: Some(6),
            unread_mentions: Some(2),
            unread_reactions: Some(3),
            muted: Some(true),
            unread_mark: Some(true),
        })
    }
}
