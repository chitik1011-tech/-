use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use tg_mobile_core::{
    backend_update_channel, spawn_core_with_store, storage::StoreFuture, BackendFuture,
    BackendUpdateReceiver, CoreCommand, CoreEvent, CoreState, MessageSummary, Result, StateStore,
    TelegramBackend, PERSISTENCE_FLUSH_INTERVAL_MS,
};
use tokio::time::{sleep, timeout};

#[derive(Clone, Default)]
struct CountingStore {
    state: Arc<RwLock<Option<CoreState>>>,
    saves: Arc<AtomicUsize>,
}

impl CountingStore {
    fn save_count(&self) -> usize {
        self.saves.load(Ordering::SeqCst)
    }

    fn snapshot(&self) -> Option<CoreState> {
        self.state.read().unwrap().clone()
    }
}

impl StateStore for CountingStore {
    fn load<'a>(&'a self) -> StoreFuture<'a, Option<CoreState>> {
        Box::pin(async move { Ok(self.snapshot()) })
    }

    fn save<'a>(&'a self, state: &'a CoreState) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            self.saves.fetch_add(1, Ordering::SeqCst);
            *self.state.write().unwrap() = Some(state.clone());
            Ok(())
        })
    }
}

struct UpdateOnlyBackend {
    updates: Option<BackendUpdateReceiver>,
}

impl TelegramBackend for UpdateOnlyBackend {
    fn handle<'a>(&'a mut self, _command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        self.updates.take()
    }
}

fn message(id: i64) -> MessageSummary {
    MessageSummary {
        id,
        chat_id: 123,
        sender_id: 456,
        text: format!("burst-{id}"),
        timestamp_unix: id,
        outgoing: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bursty_updates_are_coalesced_before_persistence() {
    const UPDATE_COUNT: i64 = 100;

    let (update_tx, update_rx) = backend_update_channel();
    let store = CountingStore::default();
    let core = spawn_core_with_store(
        UpdateOnlyBackend {
            updates: Some(update_rx),
        },
        store.clone(),
    );

    for id in 0..UPDATE_COUNT {
        update_tx
            .send(Ok(CoreEvent::MessageAdded(message(id))))
            .await
            .unwrap();
    }

    timeout(Duration::from_secs(2), async {
        loop {
            if core
                .state()
                .messages_for_chat(123)
                .last()
                .map(|message| message.id)
                == Some(UPDATE_COUNT - 1)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let wait = Duration::from_millis(PERSISTENCE_FLUSH_INTERVAL_MS * 2);
    sleep(wait).await;

    let saves = store.save_count();
    assert!(saves > 0, "state should be flushed periodically");
    assert!(
        saves < UPDATE_COUNT as usize / 2,
        "burst should be coalesced; observed {saves} saves for {UPDATE_COUNT} updates"
    );

    core.shutdown().await.unwrap();

    timeout(Duration::from_secs(2), async {
        while !core.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let persisted = store.snapshot().unwrap();
    assert!(!persisted.running);
    assert_eq!(
        persisted.messages_for_chat(123).last().unwrap().id,
        UPDATE_COUNT - 1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_flushes_dirty_state_before_periodic_tick_is_required() -> Result<()> {
    let (update_tx, update_rx) = backend_update_channel();
    let store = CountingStore::default();
    let core = spawn_core_with_store(
        UpdateOnlyBackend {
            updates: Some(update_rx),
        },
        store.clone(),
    );

    update_tx
        .send(Ok(CoreEvent::MessageAdded(message(1))))
        .await
        .unwrap();

    timeout(Duration::from_secs(1), async {
        while core.state().messages_for_chat(123).is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    core.shutdown().await?;

    timeout(Duration::from_secs(2), async {
        while !core.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let persisted = store.snapshot().expect("shutdown must flush state");
    assert!(!persisted.running);
    assert_eq!(persisted.messages_for_chat(123)[0].id, 1);

    Ok(())
}
