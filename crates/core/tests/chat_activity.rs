use std::time::Duration;

use tg_mobile_core::{
    backend::{backend_update_channel, BackendFuture, BackendUpdateReceiver, TelegramBackend},
    spawn_core, CoreCommand, CoreEvent, CoreState,
};
use tokio::time::timeout;

struct ActivityBackend {
    updates: Option<BackendUpdateReceiver>,
}

impl TelegramBackend for ActivityBackend {
    fn handle<'a>(&'a mut self, _command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        self.updates.take()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_broadcasts_activity_expiry_without_mutating_core_state() {
    let (updates, update_rx) = backend_update_channel();
    let core = spawn_core(ActivityBackend {
        updates: Some(update_rx),
    });
    let mut events = core.subscribe();

    updates
        .send(Ok(CoreEvent::ChatActivityChanged {
            chat_id: 42,
            user_id: 7,
            activity: tg_mobile_core::ChatActivityKind::Typing,
            ttl_ms: 30,
        }))
        .await
        .expect("runtime is listening to backend updates");

    assert_eq!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("activity event arrives")
            .expect("event stream remains open"),
        CoreEvent::ChatActivityChanged {
            chat_id: 42,
            user_id: 7,
            activity: tg_mobile_core::ChatActivityKind::Typing,
            ttl_ms: 30,
        }
    );

    assert_eq!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("expiry is emitted by the runtime")
            .expect("event stream remains open"),
        CoreEvent::ChatActivityExpired {
            chat_id: 42,
            user_id: 7,
        }
    );
    assert_eq!(core.state(), CoreState::default());

    core.send(CoreCommand::Shutdown)
        .await
        .expect("runtime accepts shutdown");
}
