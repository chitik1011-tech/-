use std::time::Duration;

use tg_mobile_core::{
    decode_command, spawn_core, spawn_core_with_store, AuthorizationState, BackendFuture,
    CoreCommand, CoreEvent, CoreState, MemoryStateStore, MessageHistoryState, MessageSummary,
    StateStore, TelegramBackend,
};
use tokio::time::timeout;

fn message(id: i64, timestamp_unix: i64) -> MessageSummary {
    MessageSummary {
        id,
        chat_id: 7,
        sender_id: 42,
        text: format!("message-{id}"),
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
fn page_command_is_additive_and_validates_cursor() {
    let legacy =
        decode_command(r#"{"type":"load_messages","payload":{"chat_id":7,"limit":50}}"#).unwrap();
    assert_eq!(legacy.kind(), "load_messages");

    let page = decode_command(
        r#"{"type":"load_messages_page","payload":{"chat_id":7,"before_message_id":123,"limit":50}}"#,
    )
    .unwrap();
    assert_eq!(page.kind(), "load_messages_page");
    assert!(page.validate().is_ok());

    assert!(CoreCommand::LoadMessagesPage {
        chat_id: 7,
        before_message_id: Some(0),
        limit: 50,
    }
    .validate()
    .is_err());
}

#[test]
fn matching_pages_merge_chronologically_and_advance_cursor() {
    let mut state = CoreState::default();

    state.reduce(&CoreEvent::MessagesPageLoading {
        chat_id: 7,
        before_message_id: None,
    });
    state.reduce(&CoreEvent::MessagesPageLoaded {
        chat_id: 7,
        before_message_id: None,
        messages: vec![message(4, 400), message(5, 500)],
        next_before_message_id: Some(4),
        has_more: true,
    });

    let history = state.message_history_for_chat(7).unwrap();
    assert!(!history.loading);
    assert!(history.has_more);
    assert_eq!(history.next_before_message_id, Some(4));
    assert_eq!(
        state
            .messages_for_chat(7)
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![4, 5]
    );

    state.reduce(&CoreEvent::MessagesPageLoading {
        chat_id: 7,
        before_message_id: Some(4),
    });
    state.reduce(&CoreEvent::MessagesPageLoaded {
        chat_id: 7,
        before_message_id: Some(4),
        messages: vec![message(2, 200), message(3, 300)],
        next_before_message_id: None,
        has_more: false,
    });

    let history = state.message_history_for_chat(7).unwrap();
    assert!(!history.loading);
    assert!(!history.has_more);
    assert_eq!(history.next_before_message_id, None);
    assert_eq!(
        state
            .messages_for_chat(7)
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );
}

#[test]
fn deleted_message_is_not_resurrected_by_an_in_flight_page() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessagesPageLoading {
        chat_id: 7,
        before_message_id: None,
    });

    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: Some(7),
        message_ids: vec![7],
    });
    assert!(state.deleted_message_ids[&7].contains(&7));

    state.reduce(&CoreEvent::MessagesPageLoaded {
        chat_id: 7,
        before_message_id: None,
        messages: vec![message(6, 600), message(7, 700)],
        next_before_message_id: Some(5),
        has_more: true,
    });

    assert_eq!(
        state
            .messages_for_chat(7)
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![6]
    );
    // The next request is strictly older than the deleted ID, so its tombstone
    // no longer needs to remain in persisted state.
    assert!(!state.deleted_message_ids.contains_key(&7));
}

#[test]
fn old_persisted_state_defaults_deleted_message_tombstones_to_empty() {
    let mut value = serde_json::to_value(CoreState::default()).unwrap();
    value.as_object_mut().unwrap().remove("deleted_message_ids");

    let restored: CoreState = serde_json::from_value(value).unwrap();
    assert!(restored.deleted_message_ids.is_empty());
}

#[test]
fn repeated_page_failures_clear_tombstones_and_retry_response_stays_protected() {
    let mut state = CoreState::default();

    for message_id in 1..=3 {
        state.reduce(&CoreEvent::MessagesPageLoading {
            chat_id: 7,
            before_message_id: None,
        });
        state.reduce(&CoreEvent::MessagesDeleted {
            chat_id: Some(7),
            message_ids: vec![message_id],
        });
        assert!(state.deleted_message_ids[&7].contains(&message_id));

        state.reduce(&CoreEvent::MessagesPageLoadFailed {
            chat_id: 7,
            before_message_id: None,
            error: "temporary network failure".into(),
        });
        assert!(!state.deleted_message_ids.contains_key(&7));

        // A late completion from the failed attempt is rejected after the
        // matching failure closes its loading state.
        state.reduce(&CoreEvent::MessagesPageLoaded {
            chat_id: 7,
            before_message_id: None,
            messages: vec![message(message_id, message_id * 100)],
            next_before_message_id: None,
            has_more: false,
        });
        assert!(state.messages_for_chat(7).is_empty());
    }

    state.reduce(&CoreEvent::MessagesPageLoading {
        chat_id: 7,
        before_message_id: None,
    });
    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: Some(7),
        message_ids: vec![9],
    });
    state.reduce(&CoreEvent::MessagesPageLoaded {
        chat_id: 7,
        before_message_id: None,
        messages: vec![message(9, 900), message(10, 1_000)],
        next_before_message_id: None,
        has_more: false,
    });

    assert_eq!(
        state
            .messages_for_chat(7)
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![10]
    );
    assert!(!state.deleted_message_ids.contains_key(&7));
}

#[test]
fn completing_one_chat_page_does_not_clear_another_chats_tombstones() {
    let mut state = CoreState::default();
    for chat_id in [7, 8] {
        state.reduce(&CoreEvent::MessagesPageLoading {
            chat_id,
            before_message_id: None,
        });
        state.reduce(&CoreEvent::MessagesDeleted {
            chat_id: Some(chat_id),
            message_ids: vec![chat_id],
        });
    }

    state.reduce(&CoreEvent::MessagesPageLoadFailed {
        chat_id: 7,
        before_message_id: None,
        error: "temporary network failure".into(),
    });
    assert!(!state.deleted_message_ids.contains_key(&7));
    assert!(state.deleted_message_ids[&8].contains(&8));

    let mut other_chat_deleted = message(8, 800);
    other_chat_deleted.chat_id = 8;
    let mut other_chat_kept = message(9, 900);
    other_chat_kept.chat_id = 8;
    state.reduce(&CoreEvent::MessagesPageLoaded {
        chat_id: 8,
        before_message_id: None,
        messages: vec![other_chat_deleted, other_chat_kept],
        next_before_message_id: None,
        has_more: false,
    });

    assert_eq!(
        state
            .messages_for_chat(8)
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![9]
    );
    assert!(state.deleted_message_ids.is_empty());
}

#[test]
fn older_page_survives_merge_into_a_full_message_cache() {
    let mut state = CoreState::default();
    let recent_window = (513..=1024)
        .map(|id| message(id, id * 100))
        .collect::<Vec<_>>();
    state.reduce(&CoreEvent::MessagesLoaded {
        chat_id: 7,
        messages: recent_window,
    });
    assert_eq!(state.messages_for_chat(7).len(), 512);

    state.reduce(&CoreEvent::MessagesPageLoading {
        chat_id: 7,
        before_message_id: Some(513),
    });
    let older_page = (493..=512)
        .map(|id| message(id, id * 100))
        .collect::<Vec<_>>();
    state.reduce(&CoreEvent::MessagesPageLoaded {
        chat_id: 7,
        before_message_id: Some(513),
        messages: older_page,
        next_before_message_id: Some(493),
        has_more: true,
    });

    let cached_ids = state
        .messages_for_chat(7)
        .iter()
        .map(|message| message.id)
        .collect::<Vec<_>>();
    assert_eq!(cached_ids.len(), 512);
    assert_eq!(&cached_ids[..20], &(493..=512).collect::<Vec<_>>());
    assert_eq!(cached_ids.last(), Some(&1004));
    assert!(state
        .messages_for_chat(7)
        .iter()
        .all(|message| message.id != 1005));

    let history = state.message_history_for_chat(7).unwrap();
    assert!(history.has_more);
    assert_eq!(history.next_before_message_id, Some(493));
}

#[test]
fn older_page_rejects_a_cursor_that_moves_forward_or_repeats() {
    for next_before_message_id in [Some(10), Some(11)] {
        let mut state = CoreState::default();
        state.reduce(&CoreEvent::MessagesPageLoading {
            chat_id: 7,
            before_message_id: Some(10),
        });
        state.reduce(&CoreEvent::MessagesPageLoaded {
            chat_id: 7,
            before_message_id: Some(10),
            messages: vec![message(8, 800), message(9, 900)],
            next_before_message_id,
            has_more: true,
        });

        let history = state.message_history_for_chat(7).unwrap();
        assert!(!history.has_more);
        assert_eq!(history.next_before_message_id, None);
    }
}

#[test]
fn stale_page_completion_cannot_replace_active_cursor() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessagesPageLoading {
        chat_id: 7,
        before_message_id: Some(10),
    });

    state.reduce(&CoreEvent::MessagesPageLoaded {
        chat_id: 7,
        before_message_id: Some(99),
        messages: vec![message(1, 100)],
        next_before_message_id: None,
        has_more: false,
    });

    let history = state.message_history_for_chat(7).unwrap();
    assert!(history.loading);
    assert_eq!(history.requested_before_message_id, Some(10));
    assert!(state.messages_for_chat(7).is_empty());
}

#[test]
fn failed_older_page_keeps_continuation_cursor_retryable() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessagesPageLoading {
        chat_id: 7,
        before_message_id: None,
    });
    state.reduce(&CoreEvent::MessagesPageLoaded {
        chat_id: 7,
        before_message_id: None,
        messages: vec![message(10, 1000)],
        next_before_message_id: Some(10),
        has_more: true,
    });

    state.reduce(&CoreEvent::MessagesPageLoading {
        chat_id: 7,
        before_message_id: Some(10),
    });
    state.reduce(&CoreEvent::MessagesPageLoadFailed {
        chat_id: 7,
        before_message_id: Some(10),
        error: "temporary failure".into(),
    });

    let history = state.message_history_for_chat(7).unwrap();
    assert!(!history.loading);
    assert!(history.has_more);
    assert_eq!(history.next_before_message_id, Some(10));
    assert_eq!(history.error.as_deref(), Some("temporary failure"));
}

#[test]
fn legacy_state_defaults_message_pagination_map() {
    let legacy = r#"{
        "running": true,
        "account": {"authorization":"Closed","chats":[]},
        "messages": {},
        "last_error": null
    }"#;

    let state: CoreState = serde_json::from_str(legacy).unwrap();
    assert!(state.message_history.is_empty());
}

#[tokio::test]
async fn restore_clears_inflight_request_but_keeps_retry_cursor() {
    let store = MemoryStateStore::default();
    let mut persisted = CoreState::default();
    persisted.account.authorization = AuthorizationState::Ready;
    persisted.message_history.insert(
        7,
        MessageHistoryState {
            loading: true,
            requested_before_message_id: Some(10),
            next_before_message_id: Some(10),
            has_more: true,
            error: None,
        },
    );
    store.save(&persisted).await.unwrap();

    let core = spawn_core_with_store(tg_mobile_core::MockTelegramBackend::default(), store);

    timeout(Duration::from_secs(1), async {
        loop {
            let state = core.state();
            if state.account.authorization == AuthorizationState::Ready {
                let history = state.message_history_for_chat(7).unwrap();
                assert!(!history.loading);
                assert_eq!(history.requested_before_message_id, None);
                assert_eq!(history.next_before_message_id, Some(10));
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    core.shutdown().await.unwrap();
}

struct MissingCompletionBackend;

impl TelegramBackend for MissingCompletionBackend {
    fn handle<'a>(&'a mut self, _command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

#[tokio::test]
async fn runtime_never_leaves_page_loading_when_backend_omits_completion() {
    let core = spawn_core(MissingCompletionBackend);
    let mut events = core.subscribe();

    core.send(CoreCommand::LoadMessagesPage {
        chat_id: 7,
        before_message_id: None,
        limit: 20,
    })
    .await
    .unwrap();

    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::MessagesPageLoading {
            chat_id: 7,
            before_message_id: None
        }
    ));

    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::MessagesPageLoadFailed {
            chat_id: 7,
            before_message_id: None,
            ..
        }
    ));

    let history = core.state().message_history_for_chat(7).cloned().unwrap();
    assert!(!history.loading);
    assert!(history.error.is_some());

    core.shutdown().await.unwrap();
}
