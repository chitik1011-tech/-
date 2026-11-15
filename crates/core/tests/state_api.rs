use std::{
    fs, process,
    time::{SystemTime, UNIX_EPOCH},
};

use tg_mobile_core::{
    AuthorizationState, ChatSummary, CoreEvent, CoreState, JsonFileStateStore, MessageSummary,
    StateStore, MAX_CACHED_MESSAGES_PER_CHAT,
};

fn message(id: i64, timestamp_unix: i64) -> MessageSummary {
    MessageSummary {
        id,
        chat_id: 77,
        sender_id: 10,
        text: format!("message-{id}"),
        timestamp_unix,
        outgoing: false,
        service: None,
    }
}

fn temporary_path(label: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    std::env::temp_dir().join(format!(
        "tg-mobile-core-{label}-{}-{unique}.json",
        process::id()
    ))
}

#[test]
fn message_update_reorders_cache_when_timestamp_changes() {
    let mut state = CoreState::default();

    state.reduce(&CoreEvent::MessageAdded(message(1, 100)));
    state.reduce(&CoreEvent::MessageAdded(message(2, 200)));

    let mut updated = message(1, 300);
    updated.text = "updated".into();
    state.reduce(&CoreEvent::MessageAdded(updated.clone()));

    assert_eq!(state.messages_for_chat(77), &[message(2, 200), updated]);
}

#[test]
fn incoming_message_increments_unread_only_when_first_inserted() {
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        id: 77,
        title: "existing".into(),
        unread_count: 4,
        last_message: None,
    });

    let incoming = message(10, 100);
    state.reduce(&CoreEvent::MessageAdded(incoming.clone()));
    assert_eq!(state.chat(77).unwrap().unread_count, 5);

    let mut updated = incoming;
    updated.text = "edited payload".into();
    state.reduce(&CoreEvent::MessageAdded(updated));
    assert_eq!(state.chat(77).unwrap().unread_count, 5);
}

#[test]
fn outgoing_message_does_not_increment_unread() {
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        id: 77,
        title: "existing".into(),
        unread_count: 4,
        last_message: None,
    });

    let mut outgoing = message(11, 101);
    outgoing.outgoing = true;
    state.reduce(&CoreEvent::MessageAdded(outgoing));

    assert_eq!(state.chat(77).unwrap().unread_count, 4);
}

#[test]
fn unread_increment_saturates_at_u32_max() {
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        id: 77,
        title: "existing".into(),
        unread_count: u32::MAX,
        last_message: None,
    });

    state.reduce(&CoreEvent::MessageAdded(message(12, 102)));

    assert_eq!(state.chat(77).unwrap().unread_count, u32::MAX);
}

#[test]
fn message_cache_is_bounded_to_mobile_limit() {
    let mut state = CoreState::default();

    for id in 0..(MAX_CACHED_MESSAGES_PER_CHAT as i64 + 25) {
        state.reduce(&CoreEvent::MessageAdded(message(id, id)));
    }

    let cached = state.messages_for_chat(77);
    assert_eq!(cached.len(), MAX_CACHED_MESSAGES_PER_CHAT);
    assert_eq!(cached.first().unwrap().id, 25);
}

#[test]
fn state_query_helpers_return_chat_messages_and_unread_total() {
    let mut state = CoreState::default();
    state.account.chats = vec![
        ChatSummary {
            id: 1,
            title: "one".into(),
            unread_count: 3,
            last_message: None,
        },
        ChatSummary {
            id: 2,
            title: "two".into(),
            unread_count: 4,
            last_message: None,
        },
    ];

    assert_eq!(state.chat(2).unwrap().title, "two");
    assert!(state.messages_for_chat(404).is_empty());
    assert_eq!(state.total_unread_count(), 7);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_store_writes_versioned_envelope() {
    let path = temporary_path("versioned");
    let store = JsonFileStateStore::new(&path);
    let mut state = CoreState::default();
    state.account.authorization = AuthorizationState::Ready;

    store.save(&state).await.unwrap();

    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert!(value.get("state").is_some());

    let loaded = store.load().await.unwrap().unwrap();
    assert_eq!(loaded, state);

    let _ = fs::remove_file(path);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_store_reads_legacy_raw_core_state() {
    let path = temporary_path("legacy");
    let store = JsonFileStateStore::new(&path);
    let mut state = CoreState::default();
    state.account.authorization = AuthorizationState::Ready;

    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();

    let loaded = store.load().await.unwrap().unwrap();
    assert_eq!(loaded, state);

    let _ = fs::remove_file(path);
}
