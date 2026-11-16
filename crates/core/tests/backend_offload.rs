use std::sync::Arc;

use tg_mobile_core::{
    backend_update_channel, spawn_core_with_store, AuthorizationState, BackendFuture,
    BackendUpdateReceiver, BackendUpdateSender, CoreCommand, CoreEvent, MemoryStateStore,
    MessageSummary, SearchMessagesRequest, TelegramBackend,
};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::{sleep, timeout, Duration};

struct ControlledBackend {
    updates: Option<BackendUpdateReceiver>,
    update_tx: BackendUpdateSender,
    started: mpsc::UnboundedSender<CoreCommand>,
    release: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
    hang_first: bool,
    hang_shutdown: bool,
    panic_first: bool,
    page_id: i64,
}

impl ControlledBackend {
    fn new(
        hang_first: bool,
    ) -> (
        Self,
        mpsc::UnboundedReceiver<CoreCommand>,
        oneshot::Sender<()>,
    ) {
        let (update_tx, updates) = backend_update_channel();
        let (started, started_rx) = mpsc::unbounded_channel();
        let (release_tx, release_rx) = oneshot::channel();
        (
            Self {
                updates: Some(updates),
                update_tx: update_tx.clone(),
                started,
                release: Arc::new(Mutex::new(Some(release_rx))),
                hang_first,
                hang_shutdown: false,
                panic_first: false,
                page_id: 77,
            },
            started_rx,
            release_tx,
        )
    }

    fn panicking() -> (Self, mpsc::UnboundedReceiver<CoreCommand>) {
        let (mut backend, started, release) = Self::new(false);
        let _ = release.send(());
        backend.panic_first = true;
        (backend, started)
    }

    fn hanging_shutdown() -> (Self, mpsc::UnboundedReceiver<CoreCommand>) {
        let (mut backend, started, release) = Self::new(false);
        let _ = release.send(());
        backend.hang_shutdown = true;
        (backend, started)
    }
}

impl TelegramBackend for ControlledBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        let started = self.started.clone();
        let release = self.release.clone();
        let should_hang =
            self.hang_first || (self.hang_shutdown && matches!(&command, CoreCommand::Shutdown));
        self.hang_first = false;
        let should_panic = self.panic_first;
        self.panic_first = false;
        let page_id = self.page_id;
        self.page_id += 1;
        Box::pin(async move {
            let _ = started.send(command.clone());
            assert!(!should_panic, "controlled backend worker exit");
            if should_hang {
                if let Some(receiver) = release.lock().await.take() {
                    let _ = receiver.await;
                } else {
                    std::future::pending::<()>().await;
                }
            }
            if let CoreCommand::LoadMessagesPage {
                chat_id,
                before_message_id,
                ..
            } = command
            {
                if page_id > 77 {
                    sleep(Duration::from_millis(150)).await;
                }
                return Ok(vec![CoreEvent::MessagesPageLoaded {
                    chat_id,
                    before_message_id,
                    messages: vec![message(page_id, chat_id)],
                    next_before_message_id: None,
                    has_more: false,
                }]);
            }
            Ok(Vec::new())
        })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        self.updates.take()
    }
}

fn message(id: i64, chat_id: i64) -> MessageSummary {
    MessageSummary {
        id,
        chat_id,
        sender_id: 5,
        text: format!("message {id}"),
        timestamp_unix: id,
        outgoing: false,
        location: None,
        attachments: vec![],
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

async fn wait_for_command(rx: &mut mpsc::UnboundedReceiver<CoreCommand>, expected: CoreCommand) {
    timeout(Duration::from_secs(2), async {
        loop {
            if rx.recv().await.as_ref() == Some(&expected) {
                break;
            }
        }
    })
    .await
    .expect("backend command was not dispatched");
}

#[tokio::test]
async fn hanging_rpc_does_not_block_updates_or_persistence_tick() {
    let (backend, mut started, release) = ControlledBackend::new(true);
    let updates = backend.update_tx.clone();
    let store = MemoryStateStore::default();
    let core = spawn_core_with_store(backend, store.clone());
    core.send(CoreCommand::Start).await.unwrap();
    wait_for_command(&mut started, CoreCommand::Start).await;

    updates
        .send(Ok(CoreEvent::MessageAdded(message(10, 42))))
        .await
        .unwrap();
    timeout(Duration::from_secs(1), async {
        while core.state().messages_for_chat(42).is_empty() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("actor did not apply backend update");
    timeout(Duration::from_secs(2), async {
        loop {
            if store
                .snapshot()
                .unwrap()
                .is_some_and(|state| !state.messages_for_chat(42).is_empty())
            {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("persistence tick did not save update");
    let _ = release.send(());
    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_persists_reliable_send_recovery_without_waiting_for_rpc() {
    let (backend, mut started, _release) = ControlledBackend::new(true);
    let store = MemoryStateStore::default();
    let core = spawn_core_with_store(backend, store.clone());
    core.send(CoreCommand::SendTextReliable {
        client_id: 9,
        chat_id: 42,
        text: "hello".into(),
    })
    .await
    .unwrap();
    wait_for_command(
        &mut started,
        CoreCommand::SendTextReliable {
            client_id: 9,
            chat_id: 42,
            text: "hello".into(),
        },
    )
    .await;
    timeout(Duration::from_millis(500), core.shutdown())
        .await
        .expect("shutdown waited for hung RPC")
        .unwrap();
    wait_for_command(&mut started, CoreCommand::Shutdown).await;
    timeout(Duration::from_millis(500), async {
        while core.state().running {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("actor did not stop promptly");
    assert!(!core.state().running);
    let persisted = store.snapshot().unwrap().unwrap();
    assert!(matches!(
        persisted.outgoing_send(9).unwrap().state,
        tg_mobile_core::OutgoingSendState::AwaitingConfirmation { .. }
    ));
}

#[tokio::test]
async fn idle_shutdown_is_delivered_to_backend() {
    let (backend, mut started, _release) = ControlledBackend::new(false);
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    core.shutdown().await.unwrap();
    wait_for_command(&mut started, CoreCommand::Shutdown).await;
    timeout(Duration::from_secs(1), async {
        while core.state().running {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("idle backend shutdown did not finish");
}

#[tokio::test]
async fn a_hung_backend_shutdown_is_cancelled_after_its_finite_timeout() {
    let (backend, mut started) = ControlledBackend::hanging_shutdown();
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    timeout(Duration::from_millis(1_400), async {
        core.shutdown().await.unwrap();
        wait_for_command(&mut started, CoreCommand::Shutdown).await;
        while core.state().running {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("hung backend shutdown exceeded its finite timeout");
}

#[tokio::test]
async fn stale_generation_page_result_is_ignored_and_loading_is_cleared() {
    let (backend, mut started, release) = ControlledBackend::new(true);
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    core.send(CoreCommand::LoadMessagesPage {
        chat_id: 42,
        before_message_id: Some(100),
        limit: 10,
    })
    .await
    .unwrap();
    wait_for_command(
        &mut started,
        CoreCommand::LoadMessagesPage {
            chat_id: 42,
            before_message_id: Some(100),
            limit: 10,
        },
    )
    .await;
    core.send(CoreCommand::Start).await.unwrap();
    core.send(CoreCommand::LoadMessagesPage {
        chat_id: 42,
        before_message_id: Some(100),
        limit: 10,
    })
    .await
    .unwrap();
    let _ = release.send(());
    wait_for_command(&mut started, CoreCommand::Start).await;
    timeout(Duration::from_secs(1), async {
        while !core
            .state()
            .message_history_for_chat(42)
            .is_some_and(|history| history.loading)
        {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("new-generation page request did not start");
    sleep(Duration::from_millis(30)).await;
    assert!(
        core.state()
            .message_history_for_chat(42)
            .is_some_and(|history| history.loading),
        "stale result cleared the new request"
    );
    timeout(Duration::from_secs(2), async {
        loop {
            if core
                .state()
                .message_history_for_chat(42)
                .is_some_and(|history| !history.loading)
            {
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("stale result stranded page loading");
    assert_eq!(core.state().messages_for_chat(42)[0].id, 79);
    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn generation_change_clears_search_loading_state() {
    let (backend, mut started, release) = ControlledBackend::new(true);
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    let request = SearchMessagesRequest {
        request_id: 1,
        chat_id: 42,
        query: "needle".into(),
        before_message_id: None,
        limit: 10,
    };
    core.send(CoreCommand::SearchMessages(request.clone()))
        .await
        .unwrap();
    wait_for_command(&mut started, CoreCommand::SearchMessages(request.clone())).await;
    core.send(CoreCommand::Start).await.unwrap();
    let _ = release.send(());
    timeout(Duration::from_secs(2), async {
        loop {
            if core
                .state()
                .message_search
                .chat(42)
                .is_some_and(|search| search.active.is_none())
            {
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("stale result stranded search loading");
    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn backend_operations_are_dispatched_in_worker_order() {
    let (backend, mut started, release) = ControlledBackend::new(true);
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    let first = CoreCommand::SetPhoneNumber("one".into());
    let second = CoreCommand::SubmitCode("two".into());
    core.send(first.clone()).await.unwrap();
    core.send(second.clone()).await.unwrap();
    wait_for_command(&mut started, first).await;
    assert!(timeout(Duration::from_millis(80), started.recv())
        .await
        .is_err());
    let _ = release.send(());
    wait_for_command(&mut started, second).await;
    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_hung_rpc_cannot_grow_the_backend_queue_without_bound() {
    let (backend, mut started, release) = ControlledBackend::new(true);
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    core.send(CoreCommand::Start).await.unwrap();
    wait_for_command(&mut started, CoreCommand::Start).await;
    for index in 0..80 {
        core.send(CoreCommand::SetPhoneNumber(format!("phone-{index}")))
            .await
            .unwrap();
    }
    timeout(Duration::from_secs(2), async {
        while core.state().last_error.as_deref()
            != Some("temporary Telegram transport failure: backend queue is full")
        {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("full bounded queue was not reported");
    assert!(
        started.try_recv().is_err(),
        "worker started more than its single in-flight RPC"
    );
    let _ = release.send(());
    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn closed_worker_result_channel_recovers_loading_and_actor_keeps_running() {
    let (backend, mut started) = ControlledBackend::panicking();
    let updates = backend.update_tx.clone();
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    core.send(CoreCommand::LoadMessagesPage {
        chat_id: 42,
        before_message_id: Some(100),
        limit: 10,
    })
    .await
    .unwrap();
    wait_for_command(
        &mut started,
        CoreCommand::LoadMessagesPage {
            chat_id: 42,
            before_message_id: Some(100),
            limit: 10,
        },
    )
    .await;
    timeout(Duration::from_secs(1), async {
        while core
            .state()
            .message_history_for_chat(42)
            .is_none_or(|history| history.loading)
        {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("worker exit left page loading active");
    updates
        .send(Ok(CoreEvent::MessageAdded(message(91, 42))))
        .await
        .unwrap();
    timeout(Duration::from_secs(1), async {
        while core.state().messages_for_chat(42).is_empty() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("actor stopped processing updates after worker exit");
    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn reconnect_sync_is_submitted_to_backend_worker() {
    let (backend, mut started, _release) = ControlledBackend::new(false);
    let updates = backend.update_tx.clone();
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    core.send(CoreCommand::Start).await.unwrap();
    wait_for_command(&mut started, CoreCommand::Start).await;
    updates
        .send(Ok(CoreEvent::AuthorizationChanged(
            AuthorizationState::Ready,
        )))
        .await
        .unwrap();
    sleep(Duration::from_millis(20)).await;
    let emit_lifecycle = |token, event| CoreEvent::ConnectionLifecycle { token, event };
    let token = core.state().connection_machine.token();
    updates
        .send(Ok(emit_lifecycle(
            token,
            tg_mobile_core::LinkEvent::TransportConnected,
        )))
        .await
        .unwrap();
    sleep(Duration::from_millis(20)).await;
    let token = core.state().connection_machine.token();
    updates
        .send(Ok(emit_lifecycle(
            token,
            tg_mobile_core::LinkEvent::TransportLost,
        )))
        .await
        .unwrap();
    sleep(Duration::from_millis(20)).await;
    let token = core.state().connection_machine.token();
    updates
        .send(Ok(emit_lifecycle(token, tg_mobile_core::LinkEvent::Retry)))
        .await
        .unwrap();
    sleep(Duration::from_millis(20)).await;
    let token = core.state().connection_machine.token();
    updates
        .send(Ok(emit_lifecycle(
            token,
            tg_mobile_core::LinkEvent::TransportConnected,
        )))
        .await
        .unwrap();
    sleep(Duration::from_millis(20)).await;
    let token = core.state().connection_machine.token();
    updates
        .send(Ok(emit_lifecycle(
            token,
            tg_mobile_core::LinkEvent::SyncStarted,
        )))
        .await
        .unwrap();
    wait_for_command(&mut started, CoreCommand::LoadChats { limit: 100 }).await;
    core.shutdown().await.unwrap();
}
