use tokio::sync::{broadcast, mpsc};

use crate::{
    backend::TelegramBackend,
    command::CoreCommand,
    error::{CoreError, Result},
    event::CoreEvent,
};

#[derive(Clone)]
pub struct CoreHandle {
    commands: mpsc::Sender<CoreCommand>,
    events: broadcast::Sender<CoreEvent>,
}

impl CoreHandle {
    pub async fn send(&self, command: CoreCommand) -> Result<()> {
        self.commands
            .send(command)
            .await
            .map_err(|_| CoreError::CommandChannelClosed)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.events.subscribe()
    }
}

pub fn spawn_core<B: TelegramBackend>(mut backend: B) -> CoreHandle {
    let (command_tx, mut command_rx) = mpsc::channel::<CoreCommand>(128);
    let (event_tx, _) = broadcast::channel::<CoreEvent>(256);
    let event_tx_task = event_tx.clone();

    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            let stop = matches!(command, CoreCommand::Shutdown);

            match backend.handle(command).await {
                Ok(events) => {
                    for event in events {
                        let _ = event_tx_task.send(event);
                    }
                }
                Err(err) => {
                    let _ = event_tx_task.send(CoreEvent::Error(err.to_string()));
                }
            }

            if stop {
                let _ = event_tx_task.send(CoreEvent::Stopped);
                break;
            }
        }
    });

    CoreHandle {
        commands: command_tx,
        events: event_tx,
    }
}
