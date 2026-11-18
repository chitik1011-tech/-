use std::time::Duration;

use tg_mobile_core::{
    decode_command, spawn_core, spawn_core_with_store, AuthorizationState, BackendFuture,
    CoreCommand, CoreEvent, CoreState, MemoryStateStore, MessageSearchPage, MessageSummary,
    SearchMessagesRequest, StateStore, TelegramBackend, MAX_MESSAGE_SEARCH_LIMIT,
    MAX_MESSAGE_SEARCH_QUERY_BYTES,
};
use tokio::time::timeout;

fn request(request_id: u64, query: &str, before_message_id: Option<i64>) -> SearchMessagesRequest {
    SearchMessagesRequest {
        request_id,
        chat_id: 7,
        query: query.into(),
        before_message_id,
        limit: 20,
    }
}

fn message(id: i64, timestamp_unix: i64, text: &str) -> MessageSummary {
    MessageSummary {
        id,
        chat_id: 7,
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

fn page(
    request_id: u64,
    query: &str,
    requested_before_message_id: Option<i64>,
    messages: Vec<MessageSummary>,
    next_before_message_id: Option<i64>,
    has_more: bool,
) -> MessageSearchPage {
    MessageSearchPage {
        request_id,
        chat_id: 7,
        query: query.into(),
        requested_before_message_id,
        messages,
        next_before_message_id,
        has_more,
    }
}

#[test]
fn search_command_is_additive_and_validates_input() {
    let command = decode_command(
        r#"{"type":"search_messages","payload":{"request_id":1,"chat_id":7,"query":"rust","limit":20}}"#,
    )
    .unwrap();
    assert_eq!(command.kind(), "search_messages");
    assert!(command.validate().is_ok());

    let valid = request(1, " rust ", None);
    assert!(valid.validate().is_ok());
    assert!(SearchMessagesRequest {
        limit: MAX_MESSAGE_SEARCH_LIMIT,
        ..valid.clone()
    }
    .validate()
    .is_ok());

    let invalid = [
        SearchMessagesRequest {
            request_id: 0,
            ..valid.clone()
        },
        SearchMessagesRequest {
            chat_id: -7,
            ..valid.clone()
        },
        SearchMessagesRequest {
            query: " ".into(),
            ..valid.clone()
        },
        SearchMessagesRequest {
            query: "x".repeat(MAX_MESSAGE_SEARCH_QUERY_BYTES + 1),
            ..valid.clone()
        },
        SearchMessagesRequest {
            before_message_id: Some(0),
            ..valid.clone()
        },
        SearchMessagesRequest {
            limit: 0,
            ..valid.clone()
        },
        SearchMessagesRequest {
            limit: MAX_MESSAGE_SEARCH_LIMIT + 1,
            ..valid
        },
    ];
    for request in invalid {
        assert!(request.validate().is_err(), "accepted {request:?}");
    }

    let account_wide = SearchMessagesRequest {
        chat_id: 0,
        ..valid.clone()
    };
    assert!(account_wide.validate().is_ok());
    assert!(SearchMessagesRequest {
        before_message_id: Some(100),
        ..account_wide
    }
    .validate()
    .is_err());
}

#[test]
fn search_results_are_transient_and_do_not_pollute_chat_history() {
    let mut state = CoreState::default();
    let req = request(5, "needle", None);
    state.reduce(&CoreEvent::MessageSearchStarted(req.clone()));
    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        5,
        "needle",
        None,
        vec![message(30, 300, "needle"), message(20, 200, "needle"), {
            let mut other_chat = message(31, 310, "wrong chat");
            other_chat.chat_id = 8;
            other_chat
        }],
        Some(20),
        true,
    )));

    let search = state.message_search.chat(7).unwrap();
    assert_eq!(
        search
            .results
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![30, 20]
    );
    assert_eq!(search.next_before_message_id, Some(20));
    assert!(search.has_more);
    assert!(state.messages_for_chat(7).is_empty());
}

#[test]
fn account_wide_search_keeps_global_scope_separate_from_result_chat_ids() {
    let mut state = CoreState::default();
    let request = SearchMessagesRequest {
        request_id: 3,
        chat_id: 0,
        query: "needle".into(),
        before_message_id: None,
        limit: 20,
    };
    state.reduce(&CoreEvent::MessageSearchStarted(request));
    state.reduce(&CoreEvent::MessageSearchCompleted(MessageSearchPage {
        request_id: 3,
        chat_id: 0,
        query: "needle".into(),
        requested_before_message_id: None,
        messages: vec![message(45, 450, "needle in first chat"), {
            let mut other_chat = message(45, 440, "needle in second chat");
            other_chat.chat_id = 8;
            other_chat
        }],
        next_before_message_id: None,
        has_more: false,
    }));

    let search = state.message_search.chat(0).expect("global search scope");
    assert_eq!(search.results.len(), 2);
    assert_eq!(search.results[0].chat_id, 7);
    assert_eq!(search.results[1].chat_id, 8);
    assert!(state.messages.get(&7).is_none_or(Vec::is_empty));
    assert!(state.messages.get(&8).is_none_or(Vec::is_empty));
}

#[test]
fn pagination_deduplicates_and_rejects_stale_completion() {
    let mut state = CoreState::default();

    state.reduce(&CoreEvent::MessageSearchStarted(request(9, "needle", None)));
    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        9,
        "needle",
        None,
        vec![message(30, 300, "needle"), message(20, 200, "needle")],
        Some(20),
        true,
    )));

    state.reduce(&CoreEvent::MessageSearchStarted(request(
        9,
        "needle",
        Some(20),
    )));

    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        8,
        "old",
        None,
        vec![message(99, 999, "stale")],
        None,
        false,
    )));
    assert!(state.message_search.chat(7).unwrap().active.is_some());

    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        9,
        "needle",
        Some(20),
        vec![
            message(20, 200, "needle updated"),
            message(10, 100, "needle"),
        ],
        None,
        false,
    )));

    let search = state.message_search.chat(7).unwrap();
    assert!(search.active.is_none());
    assert_eq!(
        search
            .results
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![30, 20, 10]
    );
    assert_eq!(search.results[1].text, "needle updated");
    assert!(!search.has_more);
    assert_eq!(search.next_before_message_id, None);
}

#[test]
fn same_request_id_with_new_query_replaces_active_search_and_rejects_old_response() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageSearchStarted(request(
        9,
        "old query",
        None,
    )));
    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        9,
        "old query",
        None,
        vec![message(30, 300, "old result")],
        None,
        false,
    )));

    state.reduce(&CoreEvent::MessageSearchStarted(request(
        9,
        "new query",
        None,
    )));
    let search = state.message_search.chat(7).unwrap();
    assert_eq!(
        search.active.as_ref().map(|active| active.query.as_str()),
        Some("new query")
    );
    assert!(search.results.is_empty());

    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        9,
        "old query",
        None,
        vec![message(31, 310, "stale result")],
        None,
        false,
    )));
    let search = state.message_search.chat(7).unwrap();
    assert_eq!(
        search.active.as_ref().map(|active| active.query.as_str()),
        Some("new query")
    );
    assert!(search.results.is_empty());

    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        9,
        "new query",
        None,
        vec![message(40, 400, "new result")],
        None,
        false,
    )));
    let search = state.message_search.chat(7).unwrap();
    assert!(
        search.active.is_none(),
        "successful completion must clear loading"
    );
    assert_eq!(search.results.len(), 1);
    assert_eq!(search.results[0].text, "new result");
}

#[test]
fn pagination_rejects_equal_or_newer_continuation_cursor() {
    for next_cursor in [10, 20] {
        let mut state = CoreState::default();
        state.reduce(&CoreEvent::MessageSearchStarted(request(
            10,
            "needle",
            Some(10),
        )));
        state.reduce(&CoreEvent::MessageSearchCompleted(page(
            10,
            "needle",
            Some(10),
            vec![message(5, 50, "needle")],
            Some(next_cursor),
            true,
        )));

        let search = state.message_search.chat(7).unwrap();
        assert!(search.active.is_none());
        assert!(!search.has_more, "accepted cursor {next_cursor}");
        assert_eq!(search.next_before_message_id, None);
    }
}

#[test]
fn newer_request_id_blocks_late_older_search() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageSearchStarted(request(2, "new", None)));
    state.reduce(&CoreEvent::MessageSearchStarted(request(1, "old", None)));
    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        1,
        "old",
        None,
        vec![message(1, 1, "old")],
        None,
        false,
    )));

    let search = state.message_search.chat(7).unwrap();
    assert_eq!(search.current_request_id, Some(2));
    assert_eq!(search.current_query.as_deref(), Some("new"));
    assert!(search.results.is_empty());
}

#[test]
fn failure_keeps_previous_cursor_and_results_for_retry() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageSearchStarted(request(3, "needle", None)));
    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        3,
        "needle",
        None,
        vec![message(30, 300, "needle")],
        Some(30),
        true,
    )));

    state.reduce(&CoreEvent::MessageSearchStarted(request(
        3,
        "needle",
        Some(30),
    )));
    state.reduce(&CoreEvent::MessageSearchFailed {
        request_id: 3,
        chat_id: 7,
        query: "needle".into(),
        before_message_id: Some(30),
        error: "temporary network failure".into(),
    });

    let search = state.message_search.chat(7).unwrap();
    assert!(search.active.is_none());
    assert_eq!(search.next_before_message_id, Some(30));
    assert!(search.has_more);
    assert_eq!(search.results.len(), 1);
    assert_eq!(search.error.as_deref(), Some("temporary network failure"));
}

#[test]
fn empty_success_clears_loading_and_reports_an_empty_result_set() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageSearchStarted(request(
        4, "missing", None,
    )));
    assert!(state.message_search.chat(7).unwrap().active.is_some());

    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        4,
        "missing",
        None,
        Vec::new(),
        None,
        false,
    )));

    let search = state.message_search.chat(7).unwrap();
    assert!(search.active.is_none());
    assert!(search.results.is_empty());
    assert!(!search.has_more);
    assert_eq!(search.next_before_message_id, None);
    assert_eq!(search.error, None);
}

#[test]
fn retry_clears_failure_and_stale_page_or_failure_cannot_finish_it() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageSearchStarted(request(5, "needle", None)));
    state.reduce(&CoreEvent::MessageSearchFailed {
        request_id: 5,
        chat_id: 7,
        query: "needle".into(),
        before_message_id: None,
        error: "temporary network failure".into(),
    });

    state.reduce(&CoreEvent::MessageSearchStarted(request(5, "needle", None)));
    let search = state.message_search.chat(7).unwrap();
    assert!(search.active.is_some());
    assert_eq!(search.error, None);

    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        5,
        "needle",
        Some(99),
        vec![message(99, 99, "stale cursor")],
        None,
        false,
    )));
    state.reduce(&CoreEvent::MessageSearchFailed {
        request_id: 4,
        chat_id: 7,
        query: "older query".into(),
        before_message_id: None,
        error: "stale failure".into(),
    });

    let search = state.message_search.chat(7).unwrap();
    assert!(search.active.is_some());
    assert!(search.results.is_empty());
    assert_eq!(search.error, None);

    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        5,
        "needle",
        None,
        vec![message(50, 500, "needle")],
        None,
        false,
    )));

    let search = state.message_search.chat(7).unwrap();
    assert!(search.active.is_none());
    assert_eq!(search.results, vec![message(50, 500, "needle")]);
    assert_eq!(search.error, None);
}

#[test]
fn duplicate_completion_is_ignored_after_search_finishes() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageSearchStarted(request(6, "needle", None)));
    let completed = page(
        6,
        "needle",
        None,
        vec![message(50, 500, "new result")],
        None,
        false,
    );
    state.reduce(&CoreEvent::MessageSearchCompleted(completed.clone()));
    state.reduce(&CoreEvent::MessageSearchCompleted(page(
        6,
        "needle",
        None,
        vec![message(10, 100, "duplicate stale result")],
        None,
        false,
    )));

    assert_eq!(
        state.message_search.chat(7).unwrap().results,
        completed.messages
    );
}

#[test]
fn legacy_state_defaults_search_state() {
    let legacy = r#"{
        "running": true,
        "account": {"authorization":"Closed","chats":[]},
        "messages": {},
        "last_error": null
    }"#;
    let state: CoreState = serde_json::from_str(legacy).unwrap();
    assert!(state.message_search.chats.is_empty());
}

#[tokio::test]
async fn restored_search_generation_is_cleared() {
    let store = MemoryStateStore::default();
    let mut persisted = CoreState::default();
    persisted.account.authorization = AuthorizationState::Ready;
    persisted.reduce(&CoreEvent::MessageSearchStarted(request(100, "old", None)));
    persisted.reduce(&CoreEvent::MessageSearchCompleted(page(
        100,
        "old",
        None,
        vec![message(1, 1, "old")],
        None,
        false,
    )));
    store.save(&persisted).await.unwrap();

    let core = spawn_core_with_store(tg_mobile_core::MockTelegramBackend::default(), store);
    timeout(Duration::from_secs(1), async {
        loop {
            let state = core.state();
            if state.account.authorization == AuthorizationState::Ready {
                assert!(state.message_search.chats.is_empty());
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    core.shutdown().await.unwrap();
}

struct MissingSearchCompletion;

impl TelegramBackend for MissingSearchCompletion {
    fn handle<'a>(&'a mut self, _command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

#[tokio::test]
async fn runtime_guarantees_search_failure_completion() {
    let core = spawn_core(MissingSearchCompletion);
    let mut events = core.subscribe();

    core.send(CoreCommand::SearchMessages(request(1, "needle", None)))
        .await
        .unwrap();

    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::MessageSearchStarted(_)
    ));
    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::MessageSearchFailed {
            request_id: 1,
            chat_id: 7,
            ..
        }
    ));

    let state = core.state();
    let search = state.message_search.chat(7).unwrap();
    assert!(search.active.is_none());
    assert!(search.error.is_some());

    core.shutdown().await.unwrap();
}
