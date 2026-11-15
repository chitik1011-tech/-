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
    outgoing::{OutgoingMessage, OutgoingMessageCommand, OutgoingMessageStatus},
    state::CoreState,
    storage::{NoopStateStore, StateStore},
    wire::{decode_command, decode_outgoing_command, encode_state},
};

pub const PERSISTENCE_FLUSH_INTERVAL_MS: u64 = 250;

#[derive(Clone)]
pub struct CoreHandle {
    commands: mpsc::Sender<RuntimeCommand>,
    events: broadcast::Sender<CoreEvent>,
    state: watch::Receiver<CoreState>,
}

impl CoreHandle {
    pub async fn send(&self, command: CoreCommand) -> Result<()> {
        command.validate()?;

        self.commands
            .send(RuntimeCommand::Backend(command))
            .await
            .map_err(|_| CoreError::CommandChannelClosed)
    }

    pub async fn send_json(&self, input: &str) -> Result<()> {
        self.send(decode_command(input)?).await
    }

    /// Queue a text send under a stable caller-generated ID. Reuse the exact ID and payload
    /// to retry after a failed result; duplicate sending/sent requests are idempotent.
    pub async fn send_outgoing(&self, command: OutgoingMessageCommand) -> Result<()> {
        command.validate()?;
        self.commands
            .send(RuntimeCommand::SendOutgoing(command))
            .await
            .map_err(|_| CoreError::CommandChannelClosed)
    }

    /// Decode and queue the stable-ID text send wire command.
    pub async fn send_outgoing_json(&self, input: &str) -> Result<()> {
        self.send_outgoing(decode_outgoing_command(input)?).await
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
    let (command_tx, mut command_rx) = mpsc::channel::<RuntimeCommand>(128);
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
                RuntimeInput::Command(Some(RuntimeCommand::Backend(command))) => {
                    let shutdown_requested = matches!(command, CoreCommand::Shutdown);
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
                RuntimeInput::Command(Some(RuntimeCommand::SendOutgoing(command))) => {
                    persistence_dirty |= process_outgoing_message(
                        command,
                        &mut backend,
                        &store,
                        &mut reduced_state,
                        &state_tx,
                        &event_tx_task,
                    )
                    .await;
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

enum RuntimeCommand {
    Backend(CoreCommand),
    SendOutgoing(OutgoingMessageCommand),
}

enum RuntimeInput {
    Command(Option<RuntimeCommand>),
    Update(Option<Result<CoreEvent>>),
    Persist,
}

async fn next_runtime_input(
    commands: &mut mpsc::Receiver<RuntimeCommand>,
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

async fn process_outgoing_message<B, S>(
    command: OutgoingMessageCommand,
    backend: &mut B,
    store: &S,
    state: &mut CoreState,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
) -> bool
where
    B: TelegramBackend,
    S: StateStore,
{
    let request = match command.into_request() {
        Ok(request) => request,
        Err(error) => {
            apply_event(
                CoreEvent::Error(error.to_string()),
                state,
                state_tx,
                event_tx,
            );
            return true;
        }
    };

    if let Some(existing) = state.outgoing_message(&request.client_id) {
        if existing.chat_id != request.chat_id || existing.text != request.text {
            apply_event(
                CoreEvent::Error(format!(
                    "client message ID {} was already used for a different payload",
                    request.client_id
                )),
                state,
                state_tx,
                event_tx,
            );
            return true;
        }

        match existing.status {
            OutgoingMessageStatus::Sending | OutgoingMessageStatus::Sent => return false,
            OutgoingMessageStatus::Draft | OutgoingMessageStatus::Failed => {}
        }
    } else {
        apply_event(
            CoreEvent::OutgoingMessageChanged(request.clone()),
            state,
            state_tx,
            event_tx,
        );
    }

    let mut sending = request.clone();
    sending.status = OutgoingMessageStatus::Sending;
    sending.server_message_id = None;
    sending.error = None;
    apply_event(
        CoreEvent::OutgoingMessageChanged(sending.clone()),
        state,
        state_tx,
        event_tx,
    );

    // Persist the retryable ID before any network request. On process restart, a stranded
    // Sending state is converted to Failed and can be retried with the same client ID.
    persist_state(store, state, state_tx, event_tx).await;

    let result = backend
        .handle(CoreCommand::SendText {
            chat_id: request.chat_id,
            text: request.text.clone(),
        })
        .await;

    match result {
        Ok(events) => {
            let server_message_id = events.iter().find_map(|event| match event {
                CoreEvent::MessageAdded(message)
                    if message.chat_id == request.chat_id
                        && message.outgoing
                        && message.text.trim() == request.text.trim() =>
                {
                    Some(message.id)
                }
                _ => None,
            });
            let backend_error = events.iter().find_map(|event| match event {
                CoreEvent::Error(message) => Some(message.clone()),
                _ => None,
            });

            apply_events(events, state, state_tx, event_tx);

            let mut final_state = sending;
            if let Some(message) = backend_error {
                final_state.status = OutgoingMessageStatus::Failed;
                final_state.error = Some(message);
            } else {
                final_state.status = OutgoingMessageStatus::Sent;
                final_state.server_message_id = server_message_id;
            }
            apply_event(
                CoreEvent::OutgoingMessageChanged(final_state),
                state,
                state_tx,
                event_tx,
            );
        }
        Err(error) => {
            let mut failed = sending;
            failed.status = OutgoingMessageStatus::Failed;
            failed.error = Some(error.to_string());
            apply_event(
                CoreEvent::OutgoingMessageChanged(failed),
                state,
                state_tx,
                event_tx,
            );
        }
    }

    persist_state(store, state, state_tx, event_tx).await;
    true
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
