use tokio::sync::{broadcast, mpsc, watch};

use crate::{
    backend::{BackendUpdateReceiver, TelegramBackend},
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
        command.validate()?;

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

    let mut backend_updates = backend.take_updates();

    tokio::spawn(async move {
        let mut reduced_state = restore_state(&store, &state_tx, &event_tx_task).await;

        loop {
            let input = next_runtime_input(&mut command_rx, &mut backend_updates).await;

            match input {
                RuntimeInput::Command(Some(command)) => {
                    let shutdown_requested = matches!(command, CoreCommand::Shutdown);
                    let events = match backend.handle(command).await {
                        Ok(events) => events,
                        Err(error) => vec![CoreEvent::Error(error.to_string())],
                    };

                    let stopped = apply_events(
                        events,
                        &store,
                        &mut reduced_state,
                        &state_tx,
                        &event_tx_task,
                    )
                    .await;

                    if shutdown_requested || stopped {
                        if !stopped {
                            apply_event(
                                CoreEvent::Stopped,
                                &store,
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                            )
                            .await;
                        }
                        break;
                    }
                }
                RuntimeInput::Command(None) => {
                    apply_event(
                        CoreEvent::Stopped,
                        &store,
                        &mut reduced_state,
                        &state_tx,
                        &event_tx_task,
                    )
                    .await;
                    break;
                }
                RuntimeInput::Update(Some(Ok(event))) => {
                    let stopped = matches!(event, CoreEvent::Stopped);
                    apply_event(event, &store, &mut reduced_state, &state_tx, &event_tx_task).await;

                    if stopped {
                        break;
                    }
                }
                RuntimeInput::Update(Some(Err(error))) => {
                    apply_event(
                        CoreEvent::Error(error.to_string()),
                        &store,
                        &mut reduced_state,
                        &state_tx,
                        &event_tx_task,
                    )
                    .await;
                }
                RuntimeInput::Update(None) => {
                    backend_updates = None;
                }
            }
        }
    });

    CoreHandle {
        commands: command_tx,
        events: event_tx,
        state: state_rx,
    }
}

enum RuntimeInput {
    Command(Option<CoreCommand>),
    Update(Option<Result<CoreEvent>>),
}

async fn next_runtime_input(
    commands: &mut mpsc::Receiver<CoreCommand>,
    updates: &mut Option<BackendUpdateReceiver>,
) -> RuntimeInput {
    match updates.as_mut() {
        Some(updates) => {
            tokio::select! {
                command = commands.recv() => RuntimeInput::Command(command),
                update = updates.recv() => RuntimeInput::Update(update),
            }
        }
        None => RuntimeInput::Command(commands.recv().await),
    }
}

async fn restore_state<S: StateStore>(
    store: &S,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
) -> CoreState {
    match store.load().await {
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
            let _ = event_tx.send(CoreEvent::Error(message));
            state
        }
    }
}

async fn apply_events<S: StateStore>(
    events: Vec<CoreEvent>,
    store: &S,
    state: &mut CoreState,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
) -> bool {
    let mut stopped = false;

    for event in events {
        stopped |= matches!(event, CoreEvent::Stopped);
        apply_event(event, store, state, state_tx, event_tx).await;
    }

    stopped
}

async fn apply_event<S: StateStore>(
    event: CoreEvent,
    store: &S,
    state: &mut CoreState,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
) {
    state.reduce(&event);
    let _ = state_tx.send(state.clone());
    let _ = event_tx.send(event);
    persist_state(store, state, state_tx, event_tx).await;
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
