use tokio::sync::{broadcast, mpsc, watch};

use crate::{
    backend::TelegramBackend,
    command::CoreCommand,
    error::{CoreError, Result},
    event::CoreEvent,
    state::CoreState,
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

pub fn spawn_core<B: TelegramBackend>(mut backend: B) -> CoreHandle {
    let (command_tx, mut command_rx) = mpsc::channel::<CoreCommand>(128);
    let (event_tx, _) = broadcast::channel::<CoreEvent>(256);
    let event_tx_task = event_tx.clone();

    let initial_state = CoreState::default();
    let (state_tx, state_rx) = watch::channel(initial_state);
    let mut reduced_state = CoreState::default();

    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            let stop = matches!(command, CoreCommand::Shutdown);

            let events = match backend.handle(command).await {
                Ok(events) => events,
                Err(err) => vec![CoreEvent::Error(err.to_string())],
            };

            for event in events {
                reduced_state.reduce(&event);
                let _ = state_tx.send(reduced_state.clone());
                let _ = event_tx_task.send(event);
            }

            if stop {
                let event = CoreEvent::Stopped;
                reduced_state.reduce(&event);
                let _ = state_tx.send(reduced_state.clone());
                let _ = event_tx_task.send(event);
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
