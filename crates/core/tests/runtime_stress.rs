use std::time::Duration;

use tg_mobile_core::{
    backend_update_channel, spawn_core_with_store, AuthorizationState, BackendFuture,
    BackendUpdateReceiver, CoreCommand, CoreEvent, MemoryStateStore, MessageSummary, StateStore,
    TelegramBackend, MAX_CACHED_MESSAGES_PER_CHAT,
};
use tokio::time::timeout;

struct StressBackend {
    updates: Option<BackendUpdateReceiver>,
}

impl TelegramBackend for StressBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async move {
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

fn incoming_message(id: i64) -> MessageSummary {
    MessageSummary {
        id,
        chat_id: 77,
        sender_id: 9001,
        text: format!("incoming-{id}"),
        timestamp_unix: id,
        outgoing: false,
        service_message: false,
        media_caption: None,
        forwarded_from: None,
        edited: false,
        edit_timestamp_unix: None,
        reply_to: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn runtime_handles_bursty_updates_and_commands_without_losing_latest_state() {
    const UPDATE_COUNT: i64 = 700;
    const COMMAND_COUNT: usize = 64;

    let (update_tx, update_rx) = backend_update_channel();
    let store = MemoryStateStore::default();
    let core = spawn_core_with_store(
        StressBackend {
            updates: Some(update_rx),
        },
        store.clone(),
    );

    let command_core = core.clone();
    let commands = tokio::spawn(async move {
        for _ in 0..COMMAND_COUNT {
            command_core.send(CoreCommand::Start).await.unwrap();
        }
    });

    let updates = tokio::spawn(async move {
        for id in 0..UPDATE_COUNT {
            update_tx
                .send(Ok(CoreEvent::MessageAdded(incoming_message(id))))
                .await
                .unwrap();
        }
    });

    timeout(Duration::from_secs(10), async {
        commands.await.unwrap();
        updates.await.unwrap();

        loop {
            let state = core.state();
            let messages = state.messages_for_chat(77);

            if state.account.authorization == AuthorizationState::WaitingForPhoneNumber
                && messages.len() == MAX_CACHED_MESSAGES_PER_CHAT
                && messages.last().map(|message| message.id) == Some(UPDATE_COUNT - 1)
            {
                break;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let state = core.state();
    let messages = state.messages_for_chat(77);

    assert_eq!(messages.len(), MAX_CACHED_MESSAGES_PER_CHAT);
    assert_eq!(
        messages.first().unwrap().id,
        UPDATE_COUNT - MAX_CACHED_MESSAGES_PER_CHAT as i64
    );
    assert_eq!(messages.last().unwrap().id, UPDATE_COUNT - 1);

    core.shutdown().await.unwrap();

    timeout(Duration::from_secs(2), async {
        while !core.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let persisted = store.load().await.unwrap().unwrap();
    assert!(!persisted.running);
    assert_eq!(
        persisted.messages_for_chat(77).last().unwrap().id,
        UPDATE_COUNT - 1
    );
}
