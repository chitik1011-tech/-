use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use tg_mobile_core::{
    BackendFuture, CoreCommand, CoreError, CoreEvent, MessageCursor, MessageSummary,
    TelegramBackend, MAX_CACHED_MESSAGES_PER_CHAT, MAX_MESSAGE_PAGE_SIZE,
};
use tokio::time::{timeout, Duration};

struct PagedBackend {
    messages: Vec<MessageSummary>,
    largest_requested_page: Arc<AtomicUsize>,
}

impl TelegramBackend for PagedBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async move {
            match command {
                CoreCommand::LoadMessages {
                    chat_id,
                    cursor,
                    limit,
                } => {
                    self.largest_requested_page
                        .fetch_max(limit, Ordering::Relaxed);
                    let end = cursor
                        .as_ref()
                        .and_then(|value| value.as_str().parse::<usize>().ok())
                        .unwrap_or(self.messages.len())
                        .min(self.messages.len());
                    let start = end.saturating_sub(limit);
                    let messages = self.messages[start..end].to_vec();
                    let next_cursor = (start > 0).then(|| MessageCursor::new(start.to_string()));
                    let has_more = next_cursor.is_some();
                    Ok(vec![CoreEvent::MessagesLoaded {
                        chat_id,
                        cursor,
                        messages,
                        next_cursor,
                        has_more,
                    }])
                }
                _ => Ok(Vec::new()),
            }
        })
    }
}

struct EmptyBackend;

impl TelegramBackend for EmptyBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async move {
            match command {
                CoreCommand::LoadMessages { .. } => Ok(Vec::new()),
                _ => Ok(Vec::new()),
            }
        })
    }
}

fn messages(count: usize) -> Vec<MessageSummary> {
    (0..count)
        .map(|id| MessageSummary {
            id: id as i64,
            chat_id: 77,
            sender_id: 42,
            text: format!("message-{id}"),
            timestamp_unix: id as i64,
            outgoing: false,
        })
        .collect()
}

async fn wait_for_page(
    events: &mut tokio::sync::broadcast::Receiver<CoreEvent>,
    chat_id: i64,
) -> CoreEvent {
    timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.unwrap();
            if matches!(
                &event,
                CoreEvent::MessagesLoaded {
                    chat_id: loaded_chat_id,
                    ..
                } if *loaded_chat_id == chat_id
            ) {
                break event;
            }
        }
    })
    .await
    .expect("message page timed out")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paged_history_is_capped_sorted_deduplicated_and_bounded_under_load() {
    const TOTAL_MESSAGES: usize = 1_200;
    let largest_requested_page = Arc::new(AtomicUsize::new(0));
    let core = tg_mobile_core::spawn_core(PagedBackend {
        messages: messages(TOTAL_MESSAGES),
        largest_requested_page: Arc::clone(&largest_requested_page),
    });
    let mut events = core.subscribe();
    let mut cursor = None;
    let mut page_count = 0;

    loop {
        core.send(CoreCommand::LoadMessages {
            chat_id: 77,
            cursor: cursor.clone(),
            limit: MAX_MESSAGE_PAGE_SIZE * 20,
        })
        .await
        .unwrap();

        let event = wait_for_page(&mut events, 77).await;
        let history = core.state().message_history_for_chat(77);
        assert!(!history.loading);
        cursor = history.next_cursor.clone();
        page_count += 1;
        assert!(matches!(event, CoreEvent::MessagesLoaded { .. }));

        if !history.has_more {
            break;
        }
        assert!(page_count < 20, "pagination did not terminate");
    }

    let state = core.state();
    let cached = state.messages_for_chat(77);
    assert_eq!(
        largest_requested_page.load(Ordering::Relaxed),
        MAX_MESSAGE_PAGE_SIZE
    );
    assert_eq!(page_count, 12);
    assert_eq!(cached.len(), MAX_CACHED_MESSAGES_PER_CHAT);
    assert_eq!(cached.first().map(|message| message.id), Some(0));
    assert_eq!(
        cached.last().map(|message| message.id),
        Some(MAX_CACHED_MESSAGES_PER_CHAT as i64 - 1)
    );
    assert!(cached
        .windows(2)
        .all(|pair| pair[0].timestamp_unix <= pair[1].timestamp_unix));

    core.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_backend_result_clears_loading_and_marks_history_complete() {
    let core = tg_mobile_core::spawn_core(EmptyBackend);
    let mut events = core.subscribe();

    core.send(CoreCommand::LoadMessages {
        chat_id: 77,
        cursor: None,
        limit: 25,
    })
    .await
    .unwrap();

    let event = wait_for_page(&mut events, 77).await;
    assert!(matches!(
        event,
        CoreEvent::MessagesLoaded {
            chat_id: 77,
            ref messages,
            has_more: false,
            ..
        } if messages.is_empty()
    ));
    let history = core.state().message_history_for_chat(77);
    assert!(!history.loading);
    assert!(!history.has_more);

    core.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_page_emits_chat_scoped_error_and_can_be_retried() {
    struct FailingBackend;

    impl TelegramBackend for FailingBackend {
        fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
            Box::pin(async move {
                match command {
                    CoreCommand::LoadMessages { .. } => Err(CoreError::Backend("offline".into())),
                    _ => Ok(Vec::new()),
                }
            })
        }
    }

    let core = tg_mobile_core::spawn_core(FailingBackend);
    let mut events = core.subscribe();
    let cursor = MessageCursor::new("next-page");

    core.send(CoreCommand::LoadMessages {
        chat_id: 77,
        cursor: Some(cursor.clone()),
        limit: 10,
    })
    .await
    .unwrap();

    let error = timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.unwrap();
            if matches!(event, CoreEvent::MessageHistoryLoadFailed { .. }) {
                break event;
            }
        }
    })
    .await
    .unwrap();

    assert!(matches!(
        error,
        CoreEvent::MessageHistoryLoadFailed {
            chat_id: 77,
            cursor: failed_cursor,
            error: message,
        } if failed_cursor.as_ref() == Some(&cursor) && message == "backend error: offline"
    ));
    let history = core.state().message_history_for_chat(77);
    assert!(!history.loading);
    assert!(history.has_more);
    assert_eq!(history.next_cursor, Some(cursor));
    assert_eq!(history.error.as_deref(), Some("backend error: offline"));

    core.shutdown().await.unwrap();
}
