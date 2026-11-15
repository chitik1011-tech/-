use std::time::Duration;

use tokio::{
    sync::{broadcast, mpsc, watch},
    time::{interval_at, Instant, Interval, MissedTickBehavior},
};

use crate::{
    activity::{ChatActivityTracker, CHAT_ACTIVITY_EXPIRY_CHECK_INTERVAL_MS},
    backend::{BackendUpdateReceiver, TelegramBackend},
    command::CoreCommand,
    connection_machine::{LinkEvent, LinkPhase},
    error::{CoreError, Result},
    event::CoreEvent,
    send::{OutgoingSend, OutgoingSendState},
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
        let mut reconnect_sync_due = false;
        let mut activity_tracker = ChatActivityTracker::default();
        let activity_interval = Duration::from_millis(CHAT_ACTIVITY_EXPIRY_CHECK_INTERVAL_MS);
        let mut activity_tick = interval_at(Instant::now() + activity_interval, activity_interval);
        activity_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            let input = next_runtime_input(
                &mut command_rx,
                &mut backend_updates,
                &mut persistence_tick,
                persistence_dirty,
                activity_tracker.has_active(),
                &mut activity_tick,
            )
            .await;

            match input {
                RuntimeInput::Command(Some(command)) => {
                    let shutdown_requested = matches!(&command, CoreCommand::Shutdown);
                    let starting_session = matches!(&command, CoreCommand::Start);
                    if starting_session {
                        reduced_state.connection_machine.start_session();
                        reduced_state.connection_state =
                            reduced_state.connection_machine.transport_state();
                        let _ = state_tx.send(reduced_state.clone());
                        persistence_dirty = true;
                        backend.set_connection_token(reduced_state.connection_machine.token());
                    }
                    if shutdown_requested {
                        let token = reduced_state.connection_machine.token();
                        reduced_state.reduce(&CoreEvent::ConnectionLifecycle {
                            token,
                            event: LinkEvent::Shutdown,
                        });
                        let _ = state_tx.send(reduced_state.clone());
                        backend.set_connection_token(reduced_state.connection_machine.token());
                    }
                    let page_request = match &command {
                        CoreCommand::LoadMessagesPage {
                            chat_id,
                            before_message_id,
                            ..
                        } => Some((*chat_id, *before_message_id)),
                        _ => None,
                    };
                    let initial_sync_token = matches!(&command, CoreCommand::LoadChats { .. })
                        .then_some(reduced_state.connection_machine.token())
                        .filter(|_| {
                            reduced_state.connection_machine.phase() == LinkPhase::Synchronizing
                                && reduced_state.account.authorization
                                    == crate::model::AuthorizationState::Ready
                        });
                    let search_request = match &command {
                        CoreCommand::SearchMessages(request) => Some(request.clone()),
                        _ => None,
                    };
                    let mut skip_backend = false;

                    if let CoreCommand::SendTextReliable {
                        client_id,
                        chat_id,
                        text,
                    } = &command
                    {
                        if let Some(existing) = reduced_state.send_ledger.get(client_id) {
                            if !existing.same_request(*chat_id, text) {
                                apply_event(
                                    CoreEvent::Error(format!(
                                        "client message id {client_id} was reused for a different outgoing message"
                                    )),
                                    &mut reduced_state,
                                    &state_tx,
                                    &event_tx_task,
                                );
                                skip_backend = true;
                            } else if matches!(
                                existing.state,
                                OutgoingSendState::Sent { .. }
                                    | OutgoingSendState::Rejected { .. }
                                    | OutgoingSendState::Failed {
                                        retryable: false,
                                        ..
                                    }
                            ) {
                                // A duplicate command after acknowledgment returns the original
                                // result without issuing another RPC.
                                apply_event(
                                    CoreEvent::MessageSendStateChanged(existing.clone()),
                                    &mut reduced_state,
                                    &state_tx,
                                    &event_tx_task,
                                );
                                skip_backend = true;
                            }
                        }

                        if !skip_backend {
                            // Write-ahead: publish and save the idempotency key before the
                            // backend can make a network request.
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
                            if persist_send_intent(
                                &store,
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                            )
                            .await
                            {
                                persistence_dirty = false;
                            } else {
                                skip_backend = true;
                            }
                        }
                    }

                    if let Some((chat_id, before_message_id)) = page_request {
                        apply_event(
                            CoreEvent::MessagesPageLoading {
                                chat_id,
                                before_message_id,
                            },
                            &mut reduced_state,
                            &state_tx,
                            &event_tx_task,
                        );
                        persistence_dirty = true;
                    }
                    if let Some(request) = search_request.as_ref() {
                        apply_event(
                            CoreEvent::MessageSearchStarted(request.clone()),
                            &mut reduced_state,
                            &state_tx,
                            &event_tx_task,
                        );
                        persistence_dirty = true;
                    }

                    let mut events = if skip_backend {
                        Vec::new()
                    } else {
                        match backend.handle(command.clone()).await {
                            Ok(events) => events,
                            Err(error) => {
                                if starting_session {
                                    let token = reduced_state.connection_machine.token();
                                    reduced_state.reduce(&CoreEvent::ConnectionLifecycle {
                                        token,
                                        event: LinkEvent::ConnectFailed { retryable: false },
                                    });
                                    let _ = state_tx.send(reduced_state.clone());
                                    backend.set_connection_token(
                                        reduced_state.connection_machine.token(),
                                    );
                                }
                                if let Some((chat_id, before_message_id)) = page_request {
                                    vec![CoreEvent::MessagesPageLoadFailed {
                                        chat_id,
                                        before_message_id,
                                        error: error.to_string(),
                                    }]
                                } else if let Some(request) = search_request.as_ref() {
                                    vec![CoreEvent::MessageSearchFailed {
                                        request_id: request.request_id,
                                        chat_id: request.chat_id,
                                        query: request.query.clone(),
                                        before_message_id: request.before_message_id,
                                        error: error.to_string(),
                                    }]
                                } else if let CoreCommand::SendTextReliable {
                                    client_id,
                                    chat_id,
                                    text,
                                } = &command
                                {
                                    vec![CoreEvent::MessageSendStateChanged(send_error_state(
                                        *client_id,
                                        *chat_id,
                                        text.clone(),
                                        error,
                                    ))]
                                } else {
                                    vec![CoreEvent::Error(error.to_string())]
                                }
                            }
                        }
                    };

                    if let Some((chat_id, before_message_id)) = page_request {
                        let completed = events.iter().any(|event| {
                            matches!(
                                event,
                                CoreEvent::MessagesPageLoaded {
                                    chat_id: event_chat_id,
                                    before_message_id: event_before,
                                    ..
                                } | CoreEvent::MessagesPageLoadFailed {
                                    chat_id: event_chat_id,
                                    before_message_id: event_before,
                                    ..
                                } if *event_chat_id == chat_id && *event_before == before_message_id
                            )
                        });
                        if !completed {
                            let error = events
                                .iter()
                                .find_map(|event| match event {
                                    CoreEvent::Error(error) => Some(error.clone()),
                                    _ => None,
                                })
                                .unwrap_or_else(|| {
                                    "backend returned no message page completion".into()
                                });
                            events.push(CoreEvent::MessagesPageLoadFailed {
                                chat_id,
                                before_message_id,
                                error,
                            });
                        }
                    }
                    if let Some(request) = search_request.as_ref() {
                        let completed = events.iter().any(|event| match event {
                            CoreEvent::MessageSearchCompleted(page) => {
                                page.request_id == request.request_id
                                    && page.chat_id == request.chat_id
                                    && page.query.trim() == request.query.trim()
                                    && page.requested_before_message_id == request.before_message_id
                            }
                            CoreEvent::MessageSearchFailed {
                                request_id,
                                chat_id,
                                query,
                                before_message_id,
                                ..
                            } => {
                                *request_id == request.request_id
                                    && *chat_id == request.chat_id
                                    && query.trim() == request.query.trim()
                                    && *before_message_id == request.before_message_id
                            }
                            _ => false,
                        });

                        if !completed {
                            let error = events
                                .iter()
                                .find_map(|event| match event {
                                    CoreEvent::Error(error) => Some(error.clone()),
                                    _ => None,
                                })
                                .unwrap_or_else(|| {
                                    "backend returned no message search completion".into()
                                });
                            events.push(CoreEvent::MessageSearchFailed {
                                request_id: request.request_id,
                                chat_id: request.chat_id,
                                query: request.query.clone(),
                                before_message_id: request.before_message_id,
                                error,
                            });
                        }
                    }

                    if !skip_backend {
                        if let CoreCommand::SendTextReliable {
                            client_id,
                            chat_id,
                            text,
                        } = &command
                        {
                            let has_send_completion = events.iter().any(|event| {
                                matches!(
                                    event,
                                    CoreEvent::MessageSendStateChanged(send)
                                        if send.client_id == *client_id
                                            && send.same_request(*chat_id, text)
                                            && !matches!(&send.state, OutgoingSendState::Sending)
                                )
                            });
                            if !has_send_completion {
                                events.push(CoreEvent::MessageSendStateChanged(
                                    OutgoingSend::awaiting_confirmation(
                                        *client_id,
                                        *chat_id,
                                        text.clone(),
                                        "backend returned without confirming the send outcome",
                                    ),
                                ));
                            }
                        }
                    }

                    if let Some(token) = initial_sync_token {
                        let completed = events
                            .iter()
                            .any(|event| matches!(event, CoreEvent::ChatsLoaded(_)));
                        if !completed {
                            events.push(CoreEvent::ConnectionLifecycle {
                                token,
                                event: LinkEvent::ConnectFailed { retryable: false },
                            });
                        }
                    }

                    persistence_dirty |= events.iter().any(|event| !event.is_transient());

                    let previous_token = reduced_state.connection_machine.token();
                    let stopped =
                        apply_events(events, &mut reduced_state, &state_tx, &event_tx_task, &mut activity_tracker);
                    let current_token = reduced_state.connection_machine.token();
                    if current_token != previous_token {
                        backend.set_connection_token(current_token);
                    }

                    if shutdown_requested || stopped {
                        if !stopped {
                            apply_event(
                                CoreEvent::Stopped,
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                                &mut activity_tracker,
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
                        &mut activity_tracker,
                    );
                    persist_state(&store, &mut reduced_state, &state_tx, &event_tx_task).await;
                    break;
                }
                RuntimeInput::Update(Some(Ok(event))) => {
                    let Some(event) = unwrap_current_session_update(&reduced_state, event) else {
                        continue;
                    };
                    let stopped = event.is_terminal();
                    let previous_token = reduced_state.connection_machine.token();
                    let previous_phase = reduced_state.connection_machine.phase();
                    let event_is_persistent = !event.is_transient();
                    apply_event(event, &mut reduced_state, &state_tx, &event_tx_task, &mut activity_tracker);
                    let current_phase = reduced_state.connection_machine.phase();
                    if previous_phase != LinkPhase::Reconnecting
                        && current_phase == LinkPhase::Reconnecting
                    {
                        reconnect_sync_due = true;
                    }
                    let current_token = reduced_state.connection_machine.token();
                    if current_token != previous_token {
                        backend.set_connection_token(current_token);
                    }
                    if reconnect_sync_due
                        && previous_phase != LinkPhase::Synchronizing
                        && current_phase == LinkPhase::Synchronizing
                        && reduced_state.account.authorization
                            == crate::model::AuthorizationState::Ready
                    {
                        let sync_token = reduced_state.connection_machine.token();
                        let sync_result =
                            backend.handle(CoreCommand::LoadChats { limit: 100 }).await;
                        let mut dialogs_refreshed = false;
                        match sync_result {
                            Ok(sync_events) => {
                                for sync_event in sync_events {
                                    dialogs_refreshed |=
                                        matches!(sync_event, CoreEvent::ChatsLoaded(_));
                                    let tagged = CoreEvent::SessionUpdate {
                                        token: sync_token,
                                        event: Box::new(sync_event),
                                    };
                                    if let Some(sync_event) =
                                        unwrap_current_session_update(&reduced_state, tagged)
                                    {
                                        apply_event(
                                            sync_event,
                                            &mut reduced_state,
                                            &state_tx,
                                            &event_tx_task,
                                            &mut activity_tracker,
                                        );
                                    }
                                }
                            }
                            Err(error) => apply_event(
                                CoreEvent::Error(error.to_string()),
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                                &mut activity_tracker,
                            ),
                        }
                        if !dialogs_refreshed
                            && reduced_state.connection_machine.token() == sync_token
                            && reduced_state.connection_machine.phase() == LinkPhase::Synchronizing
                        {
                            let previous_token = reduced_state.connection_machine.token();
                            apply_event(
                                CoreEvent::ConnectionLifecycle {
                                    token: sync_token,
                                    event: LinkEvent::ConnectFailed { retryable: false },
                                },
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                                &mut activity_tracker,
                            );
                            let current_token = reduced_state.connection_machine.token();
                            if current_token != previous_token {
                                backend.set_connection_token(current_token);
                            }
                        }
                        reconnect_sync_due = false;
                    }
                    persistence_dirty |= event_is_persistent;

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
                        &mut activity_tracker,
                    );
                    persistence_dirty = true;
                }
                RuntimeInput::Update(None) => {
                    backend_updates = None;
                }
                RuntimeInput::ExpireActivities => {
                    for event in activity_tracker.expire(Instant::now().into_std()) {
                        apply_event(
                            event,
                            &mut reduced_state,
                            &state_tx,
                            &event_tx_task,
                            &mut activity_tracker,
                        );
                    }
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
    ExpireActivities,
}

async fn next_runtime_input(
    commands: &mut mpsc::Receiver<CoreCommand>,
    updates: &mut Option<BackendUpdateReceiver>,
    persistence_tick: &mut Interval,
    persistence_dirty: bool,
    activity_active: bool,
    activity_tick: &mut Interval,
) -> RuntimeInput {
    match updates.as_mut() {
        Some(updates) => {
            tokio::select! {
                command = commands.recv() => RuntimeInput::Command(command),
                update = updates.recv() => RuntimeInput::Update(update),
                _ = persistence_tick.tick(), if persistence_dirty => RuntimeInput::Persist,
                _ = activity_tick.tick(), if activity_active => RuntimeInput::ExpireActivities,
            }
        }
        None => {
            tokio::select! {
                command = commands.recv() => RuntimeInput::Command(command),
                _ = persistence_tick.tick(), if persistence_dirty => RuntimeInput::Persist,
                _ = activity_tick.tick(), if activity_active => RuntimeInput::ExpireActivities,
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
            state.connection_machine.reset_disconnected();
            state.connection_state = crate::connection::ConnectionState::Disconnected;
            state.message_search = Default::default();
            state.last_error = None;
            for history in state.message_history.values_mut() {
                history.loading = false;
                history.requested_before_message_id = None;
            }
            for (client_id, send) in state.outbox.clone() {
                state.send_ledger.entry(client_id).or_insert(send);
            }
            let recovering: Vec<_> = state
                .outbox
                .values()
                .filter(|send| matches!(send.state, OutgoingSendState::Sending))
                .map(|send| send.client_id)
                .collect();
            let mut recovered_any = false;
            for client_id in recovering {
                if let Some(send) = state.outbox.get(&client_id).cloned() {
                    let recovered = OutgoingSend::awaiting_confirmation(
                        send.client_id,
                        send.chat_id,
                        send.text,
                        "process restarted while the send outcome was unknown",
                    );
                    state.outbox.insert(client_id, recovered.clone());
                    state.send_ledger.insert(client_id, recovered);
                    recovered_any = true;
                }
            }
            if recovered_any {
                if let Err(error) = store.save(&state).await {
                    state.last_error = Some(error.to_string());
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

fn send_error_state(client_id: i64, chat_id: i64, text: String, error: CoreError) -> OutgoingSend {
    match error {
        CoreError::SendOutcomeUnknown(reason) => {
            OutgoingSend::awaiting_confirmation(client_id, chat_id, text, reason)
        }
        CoreError::FloodWait { seconds } => {
            OutgoingSend::flood_wait(client_id, chat_id, text, seconds)
        }
        CoreError::TransientTransport(reason) | CoreError::TransientSend(reason) => {
            OutgoingSend::failed(client_id, chat_id, text, reason, true)
        }
        CoreError::SendRejected(reason) => OutgoingSend::rejected(client_id, chat_id, text, reason),
        other => OutgoingSend::rejected(client_id, chat_id, text, other.to_string()),
    }
}

async fn persist_send_intent<S: StateStore>(
    store: &S,
    state: &mut CoreState,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
) -> bool {
    match store.save(state).await {
        Ok(()) => true,
        Err(error) => {
            let message = error.to_string();
            state.last_error = Some(message.clone());
            let _ = state_tx.send(state.clone());
            let _ = event_tx.send(CoreEvent::Error(message));
            false
        }
    }
}

fn apply_events(
    events: Vec<CoreEvent>,
    state: &mut CoreState,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
    activity_tracker: &mut ChatActivityTracker,
) -> bool {
    let mut stopped = false;

    for event in events {
        stopped |= event.is_terminal();
        apply_event(event, state, state_tx, event_tx, activity_tracker);
    }

    stopped
}

fn apply_event(
    event: CoreEvent,
    state: &mut CoreState,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
    activity_tracker: &mut ChatActivityTracker,
) {
    if matches!(event, CoreEvent::ConnectionStateChanged(_)) {
        return;
    }
    activity_tracker.apply(&event, Instant::now().into_std());
    if !event.is_transient() {
        state.reduce(&event);
        let _ = state_tx.send(state.clone());
    }
    let _ = event_tx.send(event);
}

fn unwrap_current_session_update(state: &CoreState, event: CoreEvent) -> Option<CoreEvent> {
    match event {
        CoreEvent::SessionUpdate { token, event } if token == state.connection_machine.token() => {
            Some(*event)
        }
        CoreEvent::SessionUpdate { .. } => None,
        event => Some(event),
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
