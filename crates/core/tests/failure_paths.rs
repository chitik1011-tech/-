use std::{
    fs, process,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tg_mobile_core::{
    spawn_core, spawn_core_with_store, AuthorizationState, BackendFuture, BackendUpdateReceiver,
    CoreCommand, CoreError, CoreEvent, CoreHandle, CoreState, JsonFileStateStore, MemoryStateStore,
    MessageSummary, MockTelegramBackend, TelegramBackend,
};
use tokio::{
    sync::{mpsc, oneshot},
    time::timeout,
};

struct TestBackend {
    updates: Option<BackendUpdateReceiver>,
    error: bool,
    started: Option<oneshot::Sender<()>>,
    release: Option<oneshot::Receiver<()>>,
}

impl TestBackend {
    fn new() -> Self {
        Self {
            updates: None,
            error: false,
            started: None,
            release: None,
        }
    }

    fn with_updates(updates: BackendUpdateReceiver) -> Self {
        Self {
            updates: Some(updates),
            ..Self::new()
        }
    }
}

impl TelegramBackend for TestBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async move {
            if let Some(started) = self.started.take() {
                let _ = started.send(());
            }
            if let Some(release) = self.release.take() {
                let _ = release.await;
            }
            if self.error {
                return Err(CoreError::Backend("injected backend failure".into()));
            }

            match command {
                CoreCommand::Start => Ok(vec![CoreEvent::AuthorizationChanged(
                    AuthorizationState::WaitingForPhoneNumber,
                )]),
                _ => Ok(Vec::new()),
            }
        })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        self.updates.take()
    }
}

async fn wait_for_state<F>(core: &CoreHandle, mut predicate: F) -> CoreState
where
    F: FnMut(&CoreState) -> bool,
{
    timeout(Duration::from_secs(2), async {
        loop {
            let state = core.state();
            if predicate(&state) {
                return state;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("core state did not reach the expected transition")
}

async fn wait_until_closed(core: &CoreHandle) {
    timeout(Duration::from_secs(2), async {
        while !core.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("runtime did not close its command channel");
}

fn message(id: i64, chat_id: i64, timestamp_unix: i64, text: &str) -> MessageSummary {
    MessageSummary {
        id,
        chat_id,
        sender_id: 17,
        text: text.into(),
        timestamp_unix,
        outgoing: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backend_error_is_reported_and_runtime_remains_usable() {
    let mut backend = TestBackend::new();
    backend.error = true;
    let core = spawn_core(backend);
    let mut events = core.subscribe();

    core.send(CoreCommand::Start).await.unwrap();

    let event = timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        event,
        CoreEvent::Error("backend error: injected backend failure".into())
    );
    assert_eq!(
        core.state().last_error.as_deref(),
        Some("backend error: injected backend failure")
    );

    core.send(CoreCommand::Shutdown).await.unwrap();
    wait_until_closed(&core).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closed_backend_update_stream_does_not_stop_command_processing() {
    let (updates_tx, updates_rx) = mpsc::channel(4);
    drop(updates_tx);
    let core = spawn_core(TestBackend::with_updates(updates_rx));
    let mut events = core.subscribe();

    core.send(CoreCommand::Start).await.unwrap();

    assert_eq!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::AuthorizationChanged(AuthorizationState::WaitingForPhoneNumber)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_authorization_commands_keep_a_valid_state_transition() {
    let core = spawn_core(MockTelegramBackend::default());
    let mut events = core.subscribe();

    for _ in 0..2 {
        core.send(CoreCommand::Start).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), events.recv())
                .await
                .unwrap()
                .unwrap(),
            CoreEvent::AuthorizationChanged(AuthorizationState::WaitingForPhoneNumber)
        );
    }

    core.send(CoreCommand::SetPhoneNumber("test-phone".into()))
        .await
        .unwrap();
    let _ = timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();

    for _ in 0..2 {
        core.send(CoreCommand::SubmitCode("test-code".into()))
            .await
            .unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), events.recv())
                .await
                .unwrap()
                .unwrap(),
            CoreEvent::AuthorizationChanged(AuthorizationState::Ready)
        );
    }
    assert_eq!(
        core.state().account.authorization,
        AuthorizationState::Ready
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_while_unauthorized_reports_backend_error_without_message_insert() {
    let core = spawn_core(MockTelegramBackend::default());
    let mut events = core.subscribe();

    core.send(CoreCommand::SendText {
        chat_id: 9,
        text: "hello".into(),
    })
    .await
    .unwrap();

    assert_eq!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::Error("backend error: not authorized".into())
    );
    assert!(core.state().messages_for_chat(9).is_empty());
    assert_eq!(
        core.state().last_error.as_deref(),
        Some("backend error: not authorized")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_and_reordered_message_updates_merge_in_chronological_order() {
    let (updates_tx, updates_rx) = mpsc::channel(8);
    let core = spawn_core(TestBackend::with_updates(updates_rx));

    updates_tx
        .send(Ok(CoreEvent::MessageAdded(message(8, 5, 80, "late"))))
        .await
        .unwrap();
    updates_tx
        .send(Ok(CoreEvent::MessageAdded(message(2, 5, 20, "early"))))
        .await
        .unwrap();
    updates_tx
        .send(Ok(CoreEvent::MessageAdded(message(
            8,
            5,
            80,
            "late corrected",
        ))))
        .await
        .unwrap();

    let state = wait_for_state(&core, |state| {
        state.messages_for_chat(5).len() == 2
            && state.messages_for_chat(5)[1].text == "late corrected"
    })
    .await;
    let messages = state.messages_for_chat(5);

    assert_eq!(
        messages.iter().map(|item| item.id).collect::<Vec<_>>(),
        [2, 8]
    );
    assert_eq!(messages[0].timestamp_unix, 20);
    assert_eq!(messages[1].text, "late corrected");
    assert_eq!(state.chat(5).unwrap().unread_count, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lagged_event_subscriber_can_detect_missed_broadcast_events() {
    let (updates_tx, updates_rx) = mpsc::channel(512);
    let core = spawn_core(TestBackend::with_updates(updates_rx));
    let mut subscriber = core.subscribe();

    for index in 0..400 {
        updates_tx
            .send(Ok(CoreEvent::Error(format!("failure-{index}"))))
            .await
            .unwrap();
    }

    wait_for_state(&core, |state| {
        state.last_error.as_deref() == Some("failure-399")
    })
    .await;

    assert!(matches!(
        subscriber.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) if skipped > 0
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_waits_for_in_flight_backend_command_before_stopping() {
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let mut backend = TestBackend::new();
    backend.started = Some(started_tx);
    backend.release = Some(release_rx);
    let core = spawn_core(backend);
    let mut events = core.subscribe();

    core.send(CoreCommand::Start).await.unwrap();
    timeout(Duration::from_secs(1), started_rx)
        .await
        .unwrap()
        .unwrap();

    core.send(CoreCommand::Shutdown).await.unwrap();
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));

    release_tx.send(()).unwrap();

    assert_eq!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::AuthorizationChanged(AuthorizationState::WaitingForPhoneNumber)
    );
    assert_eq!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap(),
        CoreEvent::Stopped
    );
    wait_until_closed(&core).await;
    assert!(matches!(
        core.send(CoreCommand::Start).await,
        Err(CoreError::CommandChannelClosed)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_restores_the_state_flushed_during_shutdown() {
    let store = MemoryStateStore::default();
    let mut initial = CoreState::default();
    initial.account.authorization = AuthorizationState::Ready;
    store.save(&initial).await.unwrap();

    let first_run = spawn_core_with_store(MockTelegramBackend::default(), store.clone());
    assert_eq!(
        wait_for_state(&first_run, |state| state.account.authorization
            == AuthorizationState::Ready)
        .await
        .account
        .authorization,
        AuthorizationState::Ready
    );
    first_run.shutdown().await.unwrap();
    wait_until_closed(&first_run).await;

    let restarted = spawn_core_with_store(MockTelegramBackend::default(), store);
    let restored = wait_for_state(&restarted, |state| {
        state.account.authorization == AuthorizationState::Ready
    })
    .await;
    assert!(restored.running);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn corrupt_persisted_state_surfaces_error_and_starts_with_default_state() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "tg-mobile-core-corrupt-{}-{unique}.json",
        process::id()
    ));
    fs::write(&path, "{ definitely not json").unwrap();

    let core = spawn_core_with_store(
        MockTelegramBackend::default(),
        JsonFileStateStore::new(&path),
    );

    let state = wait_for_state(&core, |state| state.last_error.is_some()).await;
    assert!(state.running);
    assert_eq!(state.account.authorization, AuthorizationState::Closed);
    assert!(state.messages.is_empty());

    core.shutdown().await.unwrap();
    wait_until_closed(&core).await;
    let _ = fs::remove_file(path);
}
