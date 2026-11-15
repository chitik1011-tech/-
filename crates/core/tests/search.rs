use std::time::Duration;

use tg_mobile_core::{
    spawn_core_with_store, AuthorizationState, CoreCommand, CoreError, CoreEvent, CoreState,
    MemoryStateStore, MessageSummary, MessagesSearchResult,
    SearchChatMetadata, SearchMessagesRequest, SearchResult, SearchScope, SearchSenderMetadata,
    MAX_MESSAGE_SEARCH_LIMIT, StateStore,
};
use tokio::time::timeout;

fn request(request_id: u64, query: &str, cursor: Option<&str>) -> SearchMessagesRequest {
    SearchMessagesRequest {
        request_id,
        scope: SearchScope::Global,
        query: query.into(),
        limit: 20,
        cursor: cursor.map(str::to_owned),
    }
}

fn result(message_id: i64, chat_id: i64, timestamp_unix: i64) -> SearchResult {
    SearchResult {
        message: MessageSummary {
            id: message_id,
            chat_id,
            sender_id: 7,
            text: format!("match {message_id}"),
            timestamp_unix,
            outgoing: false,
        },
        chat: SearchChatMetadata {
            id: chat_id,
            title: format!("chat {chat_id}"),
        },
        sender: SearchSenderMetadata {
            id: 7,
            display_name: Some("sender".into()),
        },
        snippet: format!("snippet {message_id}"),
    }
}

fn page(
    request_id: u64,
    cursor: Option<&str>,
    results: Vec<SearchResult>,
    next_cursor: Option<&str>,
    has_more: bool,
) -> MessagesSearchResult {
    MessagesSearchResult {
        request_id,
        scope: SearchScope::Global,
        query: "needle".into(),
        requested_cursor: cursor.map(str::to_owned),
        results,
        next_cursor: next_cursor.map(str::to_owned),
        has_more,
    }
}

#[test]
fn search_command_validates_query_limits_scope_and_cursor() {
    let valid = request(1, " needle ", None);
    assert!(CoreCommand::SearchMessages(valid.clone())
        .validate()
        .is_ok());
    assert!(SearchMessagesRequest {
        limit: MAX_MESSAGE_SEARCH_LIMIT,
        ..valid.clone()
    }
    .validate()
    .is_ok());

    let invalid = [
        SearchMessagesRequest {
            query: "  ".into(),
            ..valid.clone()
        },
        SearchMessagesRequest {
            limit: 0,
            ..valid.clone()
        },
        SearchMessagesRequest {
            limit: MAX_MESSAGE_SEARCH_LIMIT + 1,
            ..valid.clone()
        },
        SearchMessagesRequest {
            scope: SearchScope::Chat(0),
            ..valid.clone()
        },
        SearchMessagesRequest {
            cursor: Some(" ".into()),
            ..valid
        },
    ];
    for request in invalid {
        assert!(matches!(
            request.validate(),
            Err(CoreError::InvalidInput(_))
        ));
    }
}

#[test]
fn reducer_ignores_stale_search_response_after_newer_query() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::SearchStarted(request(1, "first", None)));
    state.reduce(&CoreEvent::SearchStarted(request(2, "needle", None)));
    state.reduce(&CoreEvent::MessagesSearchCompleted(MessagesSearchResult {
        request_id: 1,
        scope: SearchScope::Global,
        query: "first".into(),
        requested_cursor: None,
        results: vec![result(1, 4, 10)],
        next_cursor: None,
        has_more: false,
    }));
    assert!(state.message_search.global.results.is_empty());

    state.reduce(&CoreEvent::MessagesSearchCompleted(page(
        2,
        None,
        vec![result(2, 4, 20)],
        None,
        false,
    )));
    state.reduce(&CoreEvent::SearchStarted(request(1, "late", None)));
    state.reduce(&CoreEvent::MessagesSearchCompleted(MessagesSearchResult {
        request_id: 1,
        scope: SearchScope::Global,
        query: "late".into(),
        requested_cursor: None,
        results: vec![result(5, 4, 50)],
        next_cursor: None,
        has_more: false,
    }));
    assert_eq!(state.message_search.global.results[0].message.id, 2);
}

#[test]
fn reducer_deduplicates_and_sorts_paginated_results_deterministically() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::SearchStarted(request(9, "needle", None)));
    state.reduce(&CoreEvent::MessagesSearchCompleted(page(
        9,
        None,
        vec![result(3, 2, 30), result(1, 3, 20)],
        Some("next"),
        true,
    )));
    state.reduce(&CoreEvent::SearchStarted(request(
        9,
        "needle",
        Some("next"),
    )));
    state.reduce(&CoreEvent::MessagesSearchCompleted(page(
        9,
        Some("next"),
        vec![result(1, 3, 20), result(4, 1, 30)],
        None,
        false,
    )));

    let results = &state.message_search.global.results;
    assert_eq!(results.len(), 3);
    assert_eq!(
        results
            .iter()
            .map(|item| (
                item.message.timestamp_unix,
                item.message.chat_id,
                item.message.id
            ))
            .collect::<Vec<_>>(),
        vec![(30, 1, 4), (30, 2, 3), (20, 3, 1)],
    );
    assert!(!state.message_search.global.has_more);
    assert!(state.message_search.global.next_cursor.is_none());
}

#[test]
fn reducer_rejects_page_with_wrong_cursor_or_request_id() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::SearchStarted(request(
        4,
        "needle",
        Some("expected"),
    )));
    state.reduce(&CoreEvent::MessagesSearchCompleted(page(
        4,
        Some("wrong"),
        vec![result(1, 1, 1)],
        None,
        false,
    )));
    state.reduce(&CoreEvent::MessagesSearchCompleted(page(
        3,
        Some("expected"),
        vec![result(2, 1, 2)],
        None,
        false,
    )));
    assert!(state.message_search.global.results.is_empty());
}

#[test]
fn old_core_state_json_defaults_message_search_state() {
    let json = r#"{"running":true,"account":{"authorization":"Closed","chats":[]},"messages":{},"last_error":null}"#;
    let state: CoreState = serde_json::from_str(json).unwrap();
    assert_eq!(state, CoreState::default());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restore_clears_persisted_search_generation_before_new_request() {
    let store = MemoryStateStore::default();
    let mut persisted = CoreState::default();
    persisted.account.authorization = AuthorizationState::Ready;

    persisted.reduce(&CoreEvent::SearchStarted(request(100, "needle", None)));
    persisted.reduce(&CoreEvent::MessagesSearchCompleted(page(
        100,
        None,
        vec![result(1, 3, 10)],
        Some("next"),
        true,
    )));
    persisted.reduce(&CoreEvent::SearchStarted(request(
        100,
        "needle",
        Some("next"),
    )));
    store.save(&persisted).await.unwrap();

    let core = spawn_core_with_store(tg_mobile_core::MockTelegramBackend::default(), store);
    timeout(Duration::from_secs(1), async {
        while core.state().account.authorization != AuthorizationState::Ready {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert!(core.state().message_search.global.results.is_empty());
    assert!(core.state().message_search.global.active_request.is_none());

    let mut events = core.subscribe();
    core.send(CoreCommand::SearchMessages(request(1, "fresh", None)))
        .await
        .unwrap();
    let event = timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event, CoreEvent::SearchStarted(_)));
    assert_eq!(
        core.state().message_search.global.current_request_id,
        Some(1)
    );
    assert!(core.state().message_search.global.results.is_empty());
}
