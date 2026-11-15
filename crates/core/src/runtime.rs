use tokio::sync::{broadcast, mpsc, watch};

use crate::{
    backend::TelegramBackend,
    command::CoreCommand,
    error::{CoreError, Result},
    event::CoreEvent,
    state::CoreState,
    storage::{NoopStateStore, StateStore},
    wire::{decode_command, encode_state},
};

#[derive(Clone)]
pub struct CoreHandle {
    commands: mpsc::Sender<CoreCommand>,
    events: broadcast::Sender<CoreEvent>,
    state: watch::Receiver<CoreState>,
}

impl CoreHandle {
    pub async fn send(&self, command: CoreCommand) -> Result<()> {
        self.commands
            .send(command)
            .await
            .map_err(|_| CoreError::CommandChannelClosed)
    }

    pub async fn send_json(&self, input: &str) -> Result<()> {
        self.send(decode_command(input)?).await
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.events.subscribe()
    }

    pub fn state(&self) -> CoreState {
        self.state.borrow().clone()
    }

    pub fn state_json(&self) -> Result<String> {
        encode_state(&self.state())
    }

    pub fn subscribe_state(&self) -> watch::Receiver<CoreState> {
        self.state.clone()
    }
}

pub fn spawn_core<B: TelegramBackend>(backend: B) -> CoreHandle {
    spawn_core_with_store(backend, NoopStateStore)
}

pub fn spawn_core_with_store<B, S>(mut backend: B, store: S) -> CoreHandle
where
    B: TelegramBackend,
    S: StateStore,
{
    let (command_tx, mut command_rx) = mpsc::channel::<CoreCommand>(128);
    let (event_tx, _) = broadcast::channel::<CoreEvent>(256);
    let event_tx_task = event_tx.clone();

    let (state_tx, state_rx) = watch::channel(CoreState::default());

    tokio::spawn(async move {
        let mut reduced_state = match store.load().await {
            Ok(Some(mut state)) => {
                state.running = true;
                state.last_error = None;
                let _ = state_tx.send(state.clone());
                state
            }
            Ok(None) => CoreState::default(),
            Err(error) => {
                let mut state = CoreState::default();
                let message = error.to_string();
                state.last_error = Some(message.clone());
                let _ = state_tx.send(state.clone());
                let _ = event_tx_task.send(CoreEvent::Error(message));
                state
            }
        };

        while let Some(command) = command_rx.recv().await {
            let stop = matches!(command, CoreCommand::Shutdown);

            let events = match backend.handle(command).await {
                Ok(events) => events,
                Err(error) => vec![CoreEvent::Error(error.to_string())],
            };

            for event in events {
                reduced_state.reduce(&event);
                let _ = state_tx.send(reduced_state.clone());
                let _ = event_tx_task.send(event);
                persist_state(&store, &mut reduced_state, &state_tx, &event_tx_task).await;
            }

            if stop {
                let event = CoreEvent::Stopped;
                reduced_state.reduce(&event);
                let _ = state_tx.send(reduced_state.clone());
                let _ = event_tx_task.send(event);
                persist_state(&store, &mut reduced_state, &state_tx, &event_tx_task).await;
                break;
            }
        }
    });

    CoreHandle {
        commands: command_tx,
        events: event_tx,
        state: state_rx,
    }
}

async fn persist_state<S: StateStore>(
    store: &S,
    state: &mut CoreState,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
) {
    if let Err(error) = store.save(state).await {
        let message = error.to_string();
        state.last_error = Some(message.clone());
        let _ = state_tx.send(state.clone());
        let _ = event_tx.send(CoreEvent::Error(message));
    }
}
