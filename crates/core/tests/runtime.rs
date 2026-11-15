use std::{
    fs, process,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tg_mobile_core::{
    decode_command, encode_event, encode_state, spawn_core, spawn_core_with_store,
    AuthorizationState, BackendFuture, CoreCommand, CoreEvent, CoreState, JsonFileStateStore,
    MemoryStateStore, MessageSummary, MockTelegramBackend, StateStore, TelegramBackend,
};
use tokio::time::timeout;

#[test]
fn reducer_updates_authorization_and_errors() {
    let mut state = CoreState::default();

    state.reduce(&CoreEvent::AuthorizationChanged(
        AuthorizationState::WaitingForPhoneNumber,
    ));
    assert_eq!(
        state.account.authorization,
        AuthorizationState::WaitingForPhoneNumber
    );

    state.reduce(&CoreEvent::Error("boom".into()));
    assert_eq!(state.last_error.as_deref(), Some("boom"));

    state.reduce(&CoreEvent::Stopped);
    assert!(!state.running);
}

#[test]
fn reducer_deduplicates_message_updates() {
    let mut state = CoreState::default();
    let original = MessageSummary {
        id: 7,
        chat_id: 99,
        sender_id: 1,
        text: "draft".into(),
        timestamp_unix: 100,
        outgoing: true,
    };

    state.reduce(&CoreEvent::MessageAdded(original.clone()));

    let updated = MessageSummary {
        text: "confirmed".into(),
        ..original
    };
    state.reduce(&CoreEvent::MessageAdded(updated.clone()));

    let messages = state.messages.get(&99).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0], updated);
}

#[test]
fn wire_protocol_round_trips_commands_events_and_state() {
    let command_json =
        r#"{"type":"send_text","payload":{"chat_id":42,"text":"hello from client"}}"#;
    assert_eq!(
        decode_command(command_json).unwrap(),
        CoreCommand::SendText {
            chat_id: 42,
            text: "hello from client".into(),
        }
    );

    let event = CoreEvent::AuthorizationChanged(AuthorizationState::Ready);
    let encoded_event = encode_event(&event).unwrap();
    let decoded_event: CoreEvent = serde_json::from_str(&encoded_event).unwrap();
    assert_eq!(decoded_event, event);

    let state = CoreState::default();
    let encoded_state = encode_state(&state).unwrap();
    let decoded_state: CoreState = serde_json::from_str(&encoded_state).unwrap();
    assert_eq!(decoded_state, state);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn memory_store_restores_state_for_runtime() {
    let store = MemoryStateStore::default();
    let mut saved = CoreState::default();
    saved.running = false;
    saved.last_error = Some("old error".into());
    saved.account.authorization = AuthorizationState::Ready;
    saved.chat_pagination.loaded = true;
    saved.chat_pagination.next_cursor = Some("retry-me".into());
    saved.chat_pagination.loading = true;
    saved.chat_pagination.loading_cursor = Some("in-flight".into());
    store.save(&saved).await.unwrap();

    let core = spawn_core_with_store(MockTelegramBackend::default(), store);

    let restored = timeout(Duration::from_secs(1), async {
        loop {
            let state = core.state();
            if state.account.authorization == AuthorizationState::Ready {
                break state;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert!(restored.running);
    assert_eq!(restored.last_error, None);
    assert!(!restored.chat_pagination.loading);
    assert_eq!(restored.chat_pagination.loading_cursor, None);
    assert_eq!(
        restored.chat_pagination.next_cursor.as_deref(),
        Some("retry-me")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn json_file_store_round_trips_state() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("tg-mobile-core-{}-{unique}.json", process::id()));
    let store = JsonFileStateStore::new(&path);

    let mut state = CoreState::default();
    state.account.authorization = AuthorizationState::Ready;
    store.save(&state).await.unwrap();

    let loaded = store.load().await.unwrap().unwrap();
    assert_eq!(loaded, state);

    let _ = fs::remove_file(path);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn core_handle_accepts_json_commands_and_exports_state() {
    let core = spawn_core(MockTelegramBackend::default());
    let mut events = core.subscribe();

    core.send_json(r#"{"type":"start"}"#).await.unwrap();

    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::AuthorizationChanged(AuthorizationState::WaitingForPhoneNumber)
    ));

    let state_json = core.state_json().unwrap();
    let state: CoreState = serde_json::from_str(&state_json).unwrap();
    assert_eq!(
        state.account.authorization,
        AuthorizationState::WaitingForPhoneNumber
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_backend_can_authorize_load_chats_and_send() {
    let core = spawn_core(MockTelegramBackend::default());
    let mut events = core.subscribe();

    core.send(CoreCommand::Start).await.unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::AuthorizationChanged(AuthorizationState::WaitingForPhoneNumber)
    ));

    core.send(CoreCommand::SetPhoneNumber("test-phone".into()))
        .await
        .unwrap();
    let _ = timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();

    core.send(CoreCommand::SubmitCode("test-code".into()))
        .await
        .unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::AuthorizationChanged(AuthorizationState::Ready)
    ));

    core.send(CoreCommand::LoadChats { limit: 10 })
        .await
        .unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::ChatsLoaded(chats) if !chats.is_empty()
    ));

    core.send(CoreCommand::SendText {
        chat_id: 1,
        text: "hello from rust".into(),
    })
    .await
    .unwrap();

    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::MessageAdded(message) if message.text == "hello from rust"
    ));
}

#[derive(Clone, Copy)]
enum MissingPageCompletion {
    Empty,
    ErrorEvent,
}

struct MissingPageBackend(MissingPageCompletion);

impl TelegramBackend for MissingPageBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        let events = match command {
            CoreCommand::LoadChatsPage { .. } => match self.0 {
                MissingPageCompletion::Empty => Vec::new(),
                MissingPageCompletion::ErrorEvent => {
                    vec![CoreEvent::Error("page failed".into())]
                }
            },
            _ => Vec::new(),
        };
        Box::pin(async move { Ok(events) })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_completes_page_loading_when_backend_omits_a_page() {
    for completion in [
        MissingPageCompletion::Empty,
        MissingPageCompletion::ErrorEvent,
    ] {
        let core = spawn_core(MissingPageBackend(completion));
        let mut events = core.subscribe();

        core.send(CoreCommand::LoadChatsPage {
            cursor: None,
            limit: 20,
        })
        .await
        .unwrap();

        assert!(matches!(
            timeout(Duration::from_secs(1), events.recv())
                .await
                .unwrap()
                .unwrap(),
            CoreEvent::ChatsPageLoading { cursor: None }
        ));
        assert!(matches!(
            timeout(Duration::from_secs(1), events.recv())
                .await
                .unwrap()
                .unwrap(),
            CoreEvent::ChatsPageFailed {
                cursor: None,
                error: _
            }
        ));
        assert!(!core.state().chat_pagination.loading);
        assert!(core.state().chat_pagination.error.is_some());
    }
}
