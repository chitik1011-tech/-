use std::time::Duration;

use tokio::{
    sync::{broadcast, mpsc, watch},
    time::{interval_at, Instant, Interval, MissedTickBehavior},
};

use crate::{
    backend::{BackendUpdateReceiver, TelegramBackend},
    command::CoreCommand,
    error::{CoreError, Result},
    event::CoreEvent,
    state::CoreState,
    storage::{NoopStateStore, StateStore},
    wire::{decode_command, encode_state},
};

pub const PERSISTENCE_FLUSH_INTERVAL_MS: u64 = 250;

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

    pub async fn shutdown(&self) -> Result<()> {
        self.send(CoreCommand::Shutdown).await
    }

    pub fn is_closed(&self) -> bool {
        self.commands.is_closed()
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
        let flush_interval = Duration::from_millis(PERSISTENCE_FLUSH_INTERVAL_MS);
        let mut persistence_tick = interval_at(Instant::now() + flush_interval, flush_interval);
        persistence_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut persistence_dirty = false;

        loop {
            let input = next_runtime_input(
                &mut command_rx,
                &mut backend_updates,
                &mut persistence_tick,
                persistence_dirty,
            )
            .await;

            match input {
                RuntimeInput::Command(Some(command)) => {
                    let shutdown_requested = matches!(command, CoreCommand::Shutdown);
                    if let CoreCommand::SearchMessages(request) = &command {
                        apply_event(
                            CoreEvent::SearchStarted(request.clone()),
                            &mut reduced_state,
                            &state_tx,
                            &event_tx_task,
                        );
                    }
                    let events = match backend.handle(command).await {
                        Ok(events) => events,
                        Err(error) => vec![CoreEvent::Error(error.to_string())],
                    };

                    if !events.is_empty() {
                        persistence_dirty = true;
                    }

                    let stopped =
                        apply_events(events, &mut reduced_state, &state_tx, &event_tx_task);

                    if shutdown_requested || stopped {
                        if !stopped {
                            apply_event(
                                CoreEvent::Stopped,
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                            );
                            persistence_dirty = true;
                        }

                        if persistence_dirty {
                            persist_state(&store, &mut reduced_state, &state_tx, &event_tx_task)
                                .await;
                        }
                        break;
                    }
                }
                RuntimeInput::Command(None) => {
                    apply_event(
                        CoreEvent::Stopped,
                        &mut reduced_state,
                        &state_tx,
                        &event_tx_task,
                    );
                    persist_state(&store, &mut reduced_state, &state_tx, &event_tx_task).await;
                    break;
                }
                RuntimeInput::Update(Some(Ok(event))) => {
                    let stopped = event.is_terminal();
                    apply_event(event, &mut reduced_state, &state_tx, &event_tx_task);
                    persistence_dirty = true;

                    if stopped {
                        persist_state(&store, &mut reduced_state, &state_tx, &event_tx_task).await;
                        break;
                    }
                }
                RuntimeInput::Update(Some(Err(error))) => {
                    apply_event(
                        CoreEvent::Error(error.to_string()),
                        &mut reduced_state,
                        &state_tx,
                        &event_tx_task,
                    );
                    persistence_dirty = true;
                }
                RuntimeInput::Update(None) => {
                    backend_updates = None;
                }
                RuntimeInput::Persist => {
                    persist_state(&store, &mut reduced_state, &state_tx, &event_tx_task).await;
                    persistence_dirty = false;
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
    Persist,
}

async fn next_runtime_input(
    commands: &mut mpsc::Receiver<CoreCommand>,
    updates: &mut Option<BackendUpdateReceiver>,
    persistence_tick: &mut Interval,
    persistence_dirty: bool,
) -> RuntimeInput {
    match updates.as_mut() {
        Some(updates) => {
            tokio::select! {
                command = commands.recv() => RuntimeInput::Command(command),
                update = updates.recv() => RuntimeInput::Update(update),
                _ = persistence_tick.tick(), if persistence_dirty => RuntimeInput::Persist,
            }
        }
        None => {
            tokio::select! {
                command = commands.recv() => RuntimeInput::Command(command),
                _ = persistence_tick.tick(), if persistence_dirty => RuntimeInput::Persist,
            }
        }
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

fn apply_events(
    events: Vec<CoreEvent>,
    state: &mut CoreState,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
) -> bool {
    let mut stopped = false;

    for event in events {
        stopped |= event.is_terminal();
        apply_event(event, state, state_tx, event_tx);
    }

    stopped
}

fn apply_event(
    event: CoreEvent,
    state: &mut CoreState,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
) {
    state.reduce(&event);
    let _ = state_tx.send(state.clone());
    let _ = event_tx.send(event);
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
