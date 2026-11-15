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
    send::OutgoingSend,
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
                    let reliable_send = match &command {
                        CoreCommand::SendTextReliable {
                            client_id,
                            chat_id,
                            text,
                        } => Some((*client_id, *chat_id, text.clone())),
                        _ => None,
                    };

                    if let Some((client_id, chat_id, text)) = reliable_send.as_ref() {
                        match reduced_state.begin_reliable_send(*client_id, *chat_id, text.clone())
                        {
                            Ok(false) => continue,
                            Err(error) => {
                                apply_event(
                                    CoreEvent::Error(error.to_string()),
                                    &mut reduced_state,
                                    &state_tx,
                                    &event_tx_task,
                                );
                                continue;
                            }
                            Ok(true) => {}
                        }

                        if let Err(error) = store.save(&reduced_state).await {
                            let message = format!(
                                "cannot persist reliable-send intent before network request: {error}"
                            );
                            let failed = OutgoingSend::failed(
                                *client_id,
                                *chat_id,
                                text.clone(),
                                message.clone(),
                                true,
                            );
                            apply_event(
                                CoreEvent::MessageSendStateChanged(failed),
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                            );
                            apply_event(
                                CoreEvent::Error(message),
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                            );
                            persistence_dirty = true;
                            continue;
                        }

                        apply_event(
                            CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
                                *client_id,
                                *chat_id,
                                text.clone(),
                            )),
                            &mut reduced_state,
                            &state_tx,
                            &event_tx_task,
                        );
                        persistence_dirty = true;
                    }

                    let events = match backend.handle(command).await {
                        Ok(events) => events,
                        Err(error) => match reliable_send {
                            Some((client_id, chat_id, text)) => vec![
                                CoreEvent::MessageSendStateChanged(OutgoingSend::after_error(
                                    client_id, chat_id, text, &error,
                                )),
                                CoreEvent::Error(error.to_string()),
                            ],
                            None => vec![CoreEvent::Error(error.to_string())],
                        },
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
            if state.recover_inflight_sends() {
                if let Err(error) = store.save(&state).await {
                    state.last_error = Some(error.to_string());
                    let _ = event_tx.send(CoreEvent::Error(error.to_string()));
                }
            }
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::sync::oneshot;

    use crate::{
        backend::BackendFuture,
        command::CoreCommand,
        error::CoreError,
        event::CoreEvent,
        send::{OutgoingSend, OutgoingSendState},
        storage::{MemoryStateStore, StateStore},
        TelegramBackend,
    };

    use super::{restore_state, spawn_core_with_store};

    struct WriteAheadObserver {
        store: MemoryStateStore,
        observed: Option<oneshot::Sender<Option<OutgoingSendState>>>,
    }

    impl TelegramBackend for WriteAheadObserver {
        fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
            Box::pin(async move {
                if let CoreCommand::SendTextReliable {
                    client_id,
                    chat_id,
                    text,
                } = command
                {
                    let state = self.store.snapshot()?.and_then(|state| {
                        state.outbox.get(&client_id).map(|send| send.state.clone())
                    });
                    if let Some(observed) = self.observed.take() {
                        let _ = observed.send(state);
                    }
                    return Ok(vec![CoreEvent::MessageSendStateChanged(
                        OutgoingSend::sent(client_id, chat_id, text, 42),
                    )]);
                }
                Ok(Vec::new())
            })
        }
    }

    struct OneTimeoutBackend {
        calls: Arc<Mutex<Vec<i64>>>,
    }

    impl TelegramBackend for OneTimeoutBackend {
        fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
            Box::pin(async move {
                if let CoreCommand::SendTextReliable {
                    client_id,
                    chat_id,
                    text,
                } = command
                {
                    let call_number = {
                        let mut calls = self.calls.lock().unwrap();
                        calls.push(client_id);
                        calls.len()
                    };
                    if call_number == 1 {
                        return Err(CoreError::Transport("timed out after send".into()));
                    }
                    return Ok(vec![CoreEvent::MessageSendStateChanged(
                        OutgoingSend::sent(client_id, chat_id, text, 43),
                    )]);
                }
                Ok(Vec::new())
            })
        }
    }

    #[tokio::test]
    async fn reliable_send_is_persisted_before_backend_invocation() {
        let store = MemoryStateStore::default();
        let (observed_tx, observed_rx) = oneshot::channel();
        let core = spawn_core_with_store(
            WriteAheadObserver {
                store: store.clone(),
                observed: Some(observed_tx),
            },
            store,
        );

        core.send(CoreCommand::SendTextReliable {
            client_id: 33,
            chat_id: 2,
            text: "persist first".into(),
        })
        .await
        .unwrap();

        let persisted_state = tokio::time::timeout(std::time::Duration::from_secs(1), observed_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(persisted_state, Some(OutgoingSendState::Sending)));
        core.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn retry_after_transport_ambiguity_uses_the_same_client_id() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let core = spawn_core_with_store(
            OneTimeoutBackend {
                calls: calls.clone(),
            },
            MemoryStateStore::default(),
        );
        let command = CoreCommand::SendTextReliable {
            client_id: 35,
            chat_id: 2,
            text: "retry without changing identity".into(),
        };

        core.send(command.clone()).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if matches!(
                    core.state().outgoing_send(35).map(|send| &send.state),
                    Some(OutgoingSendState::AwaitingConfirmation { .. })
                ) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        core.send(command).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if matches!(
                    core.state().outgoing_send(35).map(|send| &send.state),
                    Some(OutgoingSendState::Sent { message_id: 43 })
                ) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        assert_eq!(*calls.lock().unwrap(), [35, 35]);
        core.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn restart_recovers_inflight_send_and_persists_confirmation_state() {
        let store = MemoryStateStore::default();
        let mut initial = crate::state::CoreState::default();
        initial.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
            34,
            2,
            "restart me",
        )));
        store.save(&initial).await.unwrap();

        let (state_tx, _) = tokio::sync::watch::channel(crate::state::CoreState::default());
        let (event_tx, _) = tokio::sync::broadcast::channel(4);
        let restored = restore_state(&store, &state_tx, &event_tx).await;

        assert!(matches!(
            restored.outgoing_send(34).map(|send| &send.state),
            Some(OutgoingSendState::AwaitingConfirmation { .. })
        ));
        let persisted = store.snapshot().unwrap().unwrap();
        assert!(matches!(
            persisted.outgoing_send(34).map(|send| &send.state),
            Some(OutgoingSendState::AwaitingConfirmation { .. })
        ));
    }
}
