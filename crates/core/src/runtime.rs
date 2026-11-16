use std::collections::HashMap;
use std::time::Duration;

use tokio::{
    sync::{broadcast, mpsc, watch},
    task::JoinHandle,
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
const BACKEND_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(1_000);
const BACKEND_SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_millis(1_250);

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
    let (worker_tx, mut worker_rx) = mpsc::channel::<WorkerRequest>(32);
    let (result_tx, result_rx) = mpsc::channel::<BackendResult>(32);
    let (shutdown_signal_tx, shutdown_signal_rx) = watch::channel(false);
    let mut worker = tokio::spawn(async move {
        let mut shutdown_signal_rx = shutdown_signal_rx;
        loop {
            let request = tokio::select! {
                biased;
                _ = shutdown_signal_rx.changed() => None,
                request = worker_rx.recv() => request,
            };
            let Some(request) = request else { break };
            match request {
                WorkerRequest::SetToken(token) => backend.set_connection_token(token),
                WorkerRequest::Execute {
                    op_id,
                    generation,
                    sync_required,
                    command,
                } => {
                    // Reapply the operation's generation immediately before its RPC. This
                    // keeps the backend token contract intact even if a token update could
                    // not be queued while the bounded operation channel was full.
                    backend.set_connection_token(generation);
                    let result = tokio::select! {
                        biased;
                        _ = shutdown_signal_rx.changed() => None,
                        result = backend.handle(command.clone()) => Some(result),
                    };
                    let Some(result) = result else { break };
                    if result_tx
                        .send(BackendResult {
                            op_id,
                            generation,
                            sync_required,
                            command,
                            result,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        let _ = tokio::time::timeout(
            BACKEND_SHUTDOWN_TIMEOUT,
            backend.handle(CoreCommand::Shutdown),
        )
        .await;
    });

    tokio::spawn(async move {
        let mut result_rx = Some(result_rx);
        let mut reduced_state = restore_state(&store, &state_tx, &event_tx_task).await;
        let flush_interval = Duration::from_millis(PERSISTENCE_FLUSH_INTERVAL_MS);
        let mut persistence_tick = interval_at(Instant::now() + flush_interval, flush_interval);
        persistence_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut persistence_dirty = false;
        let mut reconnect_sync_due = false;
        let mut next_op_id = 1_u64;
        let mut live_ops = HashMap::new();
        let mut activity_tracker = ChatActivityTracker::default();
        let activity_interval = Duration::from_millis(CHAT_ACTIVITY_EXPIRY_CHECK_INTERVAL_MS);
        let mut activity_tick = interval_at(Instant::now() + activity_interval, activity_interval);
        activity_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            let input = next_runtime_input(
                &mut command_rx,
                &mut backend_updates,
                &mut result_rx,
                &mut persistence_tick,
                persistence_dirty,
                activity_tracker.has_active(),
                &mut activity_tick,
            )
            .await;

            match input {
                RuntimeInput::BackendResult(Some(result)) => {
                    if live_ops.remove(&result.op_id).is_none() {
                        continue;
                    }
                    let current_generation = reduced_state.connection_machine.token();
                    if result.generation != current_generation {
                        continue;
                    }
                    let command = result.command;
                    let mut events = match result.result {
                        Ok(events) => events,
                        Err(error) if matches!(command, CoreCommand::Start) => vec![
                            CoreEvent::Error(error.to_string()),
                            CoreEvent::ConnectionLifecycle {
                                token: current_generation,
                                event: LinkEvent::ConnectFailed { retryable: false },
                            },
                        ],
                        Err(error) => operation_error_events(&command, error),
                    };
                    complete_operation_events(&command, &mut events);
                    let initial_sync = result.sync_required
                        && reduced_state.connection_machine.phase() == LinkPhase::Synchronizing
                        && reduced_state.account.authorization
                            == crate::model::AuthorizationState::Ready;
                    if initial_sync
                        && !events
                            .iter()
                            .any(|event| matches!(event, CoreEvent::ChatsLoaded(_)))
                    {
                        events.push(CoreEvent::ConnectionLifecycle {
                            token: current_generation,
                            event: LinkEvent::ConnectFailed { retryable: false },
                        });
                    }
                    persistence_dirty |= events.iter().any(|event| !event.is_transient());
                    let previous_token = reduced_state.connection_machine.token();
                    apply_events(
                        events,
                        &mut reduced_state,
                        &state_tx,
                        &event_tx_task,
                        &mut activity_tracker,
                    );
                    let current_token = reduced_state.connection_machine.token();
                    invalidate_stale_operations(
                        &mut live_ops,
                        current_token,
                        &mut reduced_state,
                        &state_tx,
                        &event_tx_task,
                        &mut activity_tracker,
                    );
                    if current_token != previous_token {
                        let _ = worker_tx.try_send(WorkerRequest::SetToken(current_token));
                    }
                }
                RuntimeInput::BackendResult(None) => {
                    result_rx = None;
                    for (_, (_, command)) in live_ops.drain() {
                        recover_interrupted_operation(
                            &command,
                            &mut reduced_state,
                            &state_tx,
                            &event_tx_task,
                            &mut activity_tracker,
                        );
                    }
                    persistence_dirty = true;
                }
                RuntimeInput::Command(Some(command)) => {
                    if let CoreCommand::VotePoll {
                        poll_id,
                        chat_id,
                        message_id,
                        option_ids,
                    } = &command
                    {
                        let validation = reduced_state
                            .poll(*poll_id)
                            .ok_or_else(|| {
                                CoreError::InvalidInput(format!(
                                    "poll {poll_id} is not present in the current state"
                                ))
                            })
                            .and_then(|poll| {
                                if poll.chat_id != *chat_id || poll.message_id != *message_id {
                                    return Err(CoreError::InvalidInput(
                                        "poll vote identity does not match the stored poll".into(),
                                    ));
                                }
                                poll.validate_vote_selection(option_ids)
                            });
                        if let Err(error) = validation {
                            apply_event(
                                CoreEvent::PollVoteFailed {
                                    poll_id: *poll_id,
                                    error: error.to_string(),
                                },
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                                &mut activity_tracker,
                            );
                            continue;
                        }
                    }

                    match &command {
                        CoreCommand::UpdateDraft(draft) => {
                            apply_event(
                                CoreEvent::DraftUpdated(draft.clone()),
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                                &mut activity_tracker,
                            );
                            persistence_dirty = true;
                            continue;
                        }
                        CoreCommand::ClearDraft {
                            chat_id,
                            expected_updated_at_unix,
                            expected_draft,
                        } => {
                            apply_event(
                                CoreEvent::DraftCleared {
                                    chat_id: *chat_id,
                                    expected_updated_at_unix: *expected_updated_at_unix,
                                    expected_draft: expected_draft.clone(),
                                },
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                                &mut activity_tracker,
                            );
                            persistence_dirty = true;
                            continue;
                        }
                        _ => {}
                    }
                    let shutdown_requested = matches!(&command, CoreCommand::Shutdown);
                    let starting_session = matches!(&command, CoreCommand::Start);
                    if shutdown_requested {
                        let token = reduced_state.connection_machine.token();
                        reduced_state.reduce(&CoreEvent::ConnectionLifecycle {
                            token,
                            event: LinkEvent::Shutdown,
                        });
                        let pending = reduced_state.outbox.values().cloned().collect::<Vec<_>>();
                        for send in pending {
                            if matches!(send.state, OutgoingSendState::Sending) {
                                apply_event(CoreEvent::MessageSendStateChanged(OutgoingSend::awaiting_confirmation(
                                    send.client_id, send.chat_id, send.text,
                                    "shutdown interrupted the backend request; delivery is unknown",
                                )), &mut reduced_state, &state_tx, &event_tx_task, &mut activity_tracker);
                            }
                        }
                        for history in reduced_state.message_history.values_mut() {
                            if history.loading {
                                history.loading = false;
                                history.requested_before_message_id = None;
                                history.error = Some("shutdown interrupted message loading".into());
                            }
                        }
                        let active_searches = reduced_state
                            .message_search
                            .chats
                            .iter()
                            .filter_map(|(chat_id, search)| {
                                search
                                    .active
                                    .as_ref()
                                    .map(|request| (*chat_id, request.clone()))
                            })
                            .collect::<Vec<_>>();
                        for (chat_id, request) in active_searches {
                            reduced_state.message_search.fail(
                                request.request_id,
                                chat_id,
                                &request.query,
                                request.before_message_id,
                                "shutdown interrupted message search",
                            );
                        }
                        let _ = state_tx.send(reduced_state.clone());
                        apply_event(
                            CoreEvent::Stopped,
                            &mut reduced_state,
                            &state_tx,
                            &event_tx_task,
                            &mut activity_tracker,
                        );
                        persist_state(&store, &mut reduced_state, &state_tx, &event_tx_task).await;
                        stop_backend_worker(&shutdown_signal_tx, &mut worker).await;
                        break;
                    }
                    if starting_session {
                        let previous_token = reduced_state.connection_machine.token();
                        reduced_state.connection_machine.start_session();
                        reduced_state.connection_state =
                            reduced_state.connection_machine.transport_state();
                        let current_token = reduced_state.connection_machine.token();
                        if current_token != previous_token {
                            invalidate_stale_operations(
                                &mut live_ops,
                                current_token,
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                                &mut activity_tracker,
                            );
                        }
                        let _ = state_tx.send(reduced_state.clone());
                        persistence_dirty = true;
                        let _ = worker_tx.try_send(WorkerRequest::SetToken(
                            reduced_state.connection_machine.token(),
                        ));
                    }
                    let page_request = match &command {
                        CoreCommand::LoadMessagesPage {
                            chat_id,
                            before_message_id,
                            ..
                        } => Some((*chat_id, *before_message_id)),
                        _ => None,
                    };
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
                                    &mut activity_tracker,
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
                                    &mut activity_tracker,
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
                                &mut activity_tracker,
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
                            &mut activity_tracker,
                        );
                        persistence_dirty = true;
                    }
                    if let Some(request) = search_request.as_ref() {
                        apply_event(
                            CoreEvent::MessageSearchStarted(request.clone()),
                            &mut reduced_state,
                            &state_tx,
                            &event_tx_task,
                            &mut activity_tracker,
                        );
                        persistence_dirty = true;
                    }

                    if skip_backend {
                        continue;
                    }
                    let op_id = next_op_id;
                    next_op_id = next_op_id.wrapping_add(1).max(1);
                    let generation = reduced_state.connection_machine.token();
                    live_ops.insert(op_id, (generation, command.clone()));
                    let sync_required = matches!(&command, CoreCommand::LoadChats { .. })
                        && reduced_state.connection_machine.phase() == LinkPhase::Synchronizing
                        && reduced_state.account.authorization
                            == crate::model::AuthorizationState::Ready;
                    if worker_tx
                        .try_send(WorkerRequest::Execute {
                            op_id,
                            generation,
                            sync_required,
                            command: command.clone(),
                        })
                        .is_err()
                    {
                        live_ops.remove(&op_id);
                        let mut events = operation_error_events(
                            &command,
                            CoreError::TransientTransport("backend queue is full".into()),
                        );
                        if starting_session {
                            events.push(CoreEvent::ConnectionLifecycle {
                                token: generation,
                                event: LinkEvent::ConnectFailed { retryable: false },
                            });
                        }
                        complete_operation_events(&command, &mut events);
                        apply_events(
                            events,
                            &mut reduced_state,
                            &state_tx,
                            &event_tx_task,
                            &mut activity_tracker,
                        );
                        persistence_dirty = true;
                    }
                    continue;
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
                    stop_backend_worker(&shutdown_signal_tx, &mut worker).await;
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
                    apply_event(
                        event,
                        &mut reduced_state,
                        &state_tx,
                        &event_tx_task,
                        &mut activity_tracker,
                    );
                    let current_phase = reduced_state.connection_machine.phase();
                    if previous_phase != LinkPhase::Reconnecting
                        && current_phase == LinkPhase::Reconnecting
                    {
                        reconnect_sync_due = true;
                    }
                    let current_token = reduced_state.connection_machine.token();
                    invalidate_stale_operations(
                        &mut live_ops,
                        current_token,
                        &mut reduced_state,
                        &state_tx,
                        &event_tx_task,
                        &mut activity_tracker,
                    );
                    if current_token != previous_token {
                        let _ = worker_tx.try_send(WorkerRequest::SetToken(current_token));
                    }
                    if reconnect_sync_due
                        && previous_phase != LinkPhase::Synchronizing
                        && current_phase == LinkPhase::Synchronizing
                        && reduced_state.account.authorization
                            == crate::model::AuthorizationState::Ready
                    {
                        let sync_token = reduced_state.connection_machine.token();
                        let op_id = next_op_id;
                        next_op_id = next_op_id.wrapping_add(1).max(1);
                        live_ops.insert(op_id, (sync_token, CoreCommand::LoadChats { limit: 100 }));
                        if worker_tx
                            .try_send(WorkerRequest::Execute {
                                op_id,
                                generation: sync_token,
                                sync_required: true,
                                command: CoreCommand::LoadChats { limit: 100 },
                            })
                            .is_err()
                        {
                            live_ops.remove(&op_id);
                            apply_event(
                                CoreEvent::Error(
                                    "backend queue is full during reconnect sync".into(),
                                ),
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                                &mut activity_tracker,
                            );
                            if reduced_state.connection_machine.token() == sync_token
                                && reduced_state.connection_machine.phase()
                                    == LinkPhase::Synchronizing
                            {
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
                            }
                        }
                        reconnect_sync_due = false;
                    }
                    persistence_dirty |= event_is_persistent;

                    if stopped {
                        persist_state(&store, &mut reduced_state, &state_tx, &event_tx_task).await;
                        stop_backend_worker(&shutdown_signal_tx, &mut worker).await;
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
    BackendResult(Option<BackendResult>),
    Persist,
    ExpireActivities,
}

async fn stop_backend_worker(signal: &watch::Sender<bool>, worker: &mut JoinHandle<()>) {
    let _ = signal.send(true);
    if tokio::time::timeout(BACKEND_SHUTDOWN_JOIN_TIMEOUT, &mut *worker)
        .await
        .is_err()
    {
        worker.abort();
    }
}

enum WorkerRequest {
    SetToken(crate::connection_machine::ConnectionToken),
    Execute {
        op_id: u64,
        generation: crate::connection_machine::ConnectionToken,
        sync_required: bool,
        command: CoreCommand,
    },
}

struct BackendResult {
    op_id: u64,
    generation: crate::connection_machine::ConnectionToken,
    sync_required: bool,
    command: CoreCommand,
    result: Result<Vec<CoreEvent>>,
}

fn operation_error_events(command: &CoreCommand, error: CoreError) -> Vec<CoreEvent> {
    match command {
        CoreCommand::LoadMessagesPage {
            chat_id,
            before_message_id,
            ..
        } => vec![CoreEvent::MessagesPageLoadFailed {
            chat_id: *chat_id,
            before_message_id: *before_message_id,
            error: error.to_string(),
        }],
        CoreCommand::SearchMessages(request) => vec![CoreEvent::MessageSearchFailed {
            request_id: request.request_id,
            chat_id: request.chat_id,
            query: request.query.clone(),
            before_message_id: request.before_message_id,
            error: error.to_string(),
        }],
        CoreCommand::SendTextReliable {
            client_id,
            chat_id,
            text,
        } => vec![CoreEvent::MessageSendStateChanged(send_error_state(
            *client_id,
            *chat_id,
            text.clone(),
            error,
        ))],
        CoreCommand::VotePoll { poll_id, .. } => vec![CoreEvent::PollVoteFailed {
            poll_id: *poll_id,
            error: error.to_string(),
        }],
        CoreCommand::ForwardMessages(request) => vec![CoreEvent::MessagesForwarded(
            crate::forwarding::ForwardMessagesResult {
                request_id: request.request_id.clone(),
                source_chat_id: request.source_chat_id,
                destination_chat_id: request.destination_chat_id,
                outcomes: request
                    .message_ids
                    .iter()
                    .map(|id| crate::forwarding::ForwardOutcome::Failed {
                        source_message_id: *id,
                        error: error.to_string(),
                    })
                    .collect(),
            },
        )],
        _ => vec![CoreEvent::Error(error.to_string())],
    }
}

fn complete_operation_events(command: &CoreCommand, events: &mut Vec<CoreEvent>) {
    if let CoreCommand::ForwardMessages(request) = command {
        *events = crate::forwarding::normalize_forward_events(request, std::mem::take(events));
    }
    if let CoreCommand::LoadMessagesPage {
        chat_id,
        before_message_id,
        ..
    } = command
    {
        if !events.iter().any(|event| matches!(event, CoreEvent::MessagesPageLoaded { chat_id: id, before_message_id: cursor, .. } | CoreEvent::MessagesPageLoadFailed { chat_id: id, before_message_id: cursor, .. } if id == chat_id && cursor == before_message_id)) {
            events.push(CoreEvent::MessagesPageLoadFailed { chat_id: *chat_id, before_message_id: *before_message_id, error: "backend returned no message page completion".into() });
        }
    }
    if let CoreCommand::SearchMessages(request) = command {
        let complete = events.iter().any(|event| matches!(event,
            CoreEvent::MessageSearchCompleted(page) if page.request_id == request.request_id && page.chat_id == request.chat_id && page.query.trim() == request.query.trim() && page.requested_before_message_id == request.before_message_id
        ) || matches!(event, CoreEvent::MessageSearchFailed { request_id, chat_id, query, before_message_id, .. } if *request_id == request.request_id && *chat_id == request.chat_id && query.trim() == request.query.trim() && *before_message_id == request.before_message_id));
        if !complete {
            events.push(CoreEvent::MessageSearchFailed {
                request_id: request.request_id,
                chat_id: request.chat_id,
                query: request.query.clone(),
                before_message_id: request.before_message_id,
                error: "backend returned no message search completion".into(),
            });
        }
    }
    if let CoreCommand::SendTextReliable {
        client_id,
        chat_id,
        text,
    } = command
    {
        if !events.iter().any(|event| matches!(event, CoreEvent::MessageSendStateChanged(send) if send.client_id == *client_id && send.same_request(*chat_id, text) && !matches!(send.state, OutgoingSendState::Sending))) {
            events.push(CoreEvent::MessageSendStateChanged(OutgoingSend::awaiting_confirmation(*client_id, *chat_id, text.clone(), "backend returned without confirming the send outcome")));
        }
    }
}

fn recover_interrupted_operation(
    command: &CoreCommand,
    state: &mut CoreState,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
    activity_tracker: &mut ChatActivityTracker,
) {
    match command {
        CoreCommand::LoadMessagesPage {
            chat_id,
            before_message_id,
            ..
        } => apply_event(
            CoreEvent::MessagesPageLoadFailed {
                chat_id: *chat_id,
                before_message_id: *before_message_id,
                error: "connection generation changed while loading messages".into(),
            },
            state,
            state_tx,
            event_tx,
            activity_tracker,
        ),
        CoreCommand::SearchMessages(request) => apply_event(
            CoreEvent::MessageSearchFailed {
                request_id: request.request_id,
                chat_id: request.chat_id,
                query: request.query.clone(),
                before_message_id: request.before_message_id,
                error: "connection generation changed while searching messages".into(),
            },
            state,
            state_tx,
            event_tx,
            activity_tracker,
        ),
        CoreCommand::SendTextReliable {
            client_id,
            chat_id,
            text,
        } => apply_event(
            CoreEvent::MessageSendStateChanged(OutgoingSend::awaiting_confirmation(
                *client_id,
                *chat_id,
                text.clone(),
                "connection changed while send outcome was unknown",
            )),
            state,
            state_tx,
            event_tx,
            activity_tracker,
        ),
        CoreCommand::ForwardMessages(request) => apply_event(
            CoreEvent::MessagesForwarded(crate::forwarding::ForwardMessagesResult {
                request_id: request.request_id.clone(),
                source_chat_id: request.source_chat_id,
                destination_chat_id: request.destination_chat_id,
                outcomes: request
                    .message_ids
                    .iter()
                    .map(|id| crate::forwarding::ForwardOutcome::Failed {
                        source_message_id: *id,
                        error: "forward outcome is unknown because the connection changed".into(),
                    })
                    .collect(),
            }),
            state,
            state_tx,
            event_tx,
            activity_tracker,
        ),
        _ => {}
    }
}

fn invalidate_stale_operations(
    operations: &mut HashMap<u64, (crate::connection_machine::ConnectionToken, CoreCommand)>,
    current_generation: crate::connection_machine::ConnectionToken,
    state: &mut CoreState,
    state_tx: &watch::Sender<CoreState>,
    event_tx: &broadcast::Sender<CoreEvent>,
    activity_tracker: &mut ChatActivityTracker,
) {
    let stale_ids = operations
        .iter()
        .filter_map(|(op_id, (generation, _))| {
            (*generation != current_generation).then_some(*op_id)
        })
        .collect::<Vec<_>>();
    for op_id in stale_ids {
        if let Some((_, command)) = operations.remove(&op_id) {
            recover_interrupted_operation(&command, state, state_tx, event_tx, activity_tracker);
        }
    }
}

async fn next_runtime_input(
    commands: &mut mpsc::Receiver<CoreCommand>,
    updates: &mut Option<BackendUpdateReceiver>,
    results: &mut Option<mpsc::Receiver<BackendResult>>,
    persistence_tick: &mut Interval,
    persistence_dirty: bool,
    activity_active: bool,
    activity_tick: &mut Interval,
) -> RuntimeInput {
    match (updates.as_mut(), results.as_mut()) {
        (Some(updates), Some(results)) => {
            tokio::select! {
                command = commands.recv() => RuntimeInput::Command(command),
                update = updates.recv() => RuntimeInput::Update(update),
                result = results.recv() => RuntimeInput::BackendResult(result),
                _ = persistence_tick.tick(), if persistence_dirty => RuntimeInput::Persist,
                _ = activity_tick.tick(), if activity_active => RuntimeInput::ExpireActivities,
            }
        }
        (None, Some(results)) => {
            tokio::select! {
                command = commands.recv() => RuntimeInput::Command(command),
                result = results.recv() => RuntimeInput::BackendResult(result),
                _ = persistence_tick.tick(), if persistence_dirty => RuntimeInput::Persist,
                _ = activity_tick.tick(), if activity_active => RuntimeInput::ExpireActivities,
            }
        }
        (Some(updates), None) => {
            tokio::select! {
                command = commands.recv() => RuntimeInput::Command(command),
                update = updates.recv() => RuntimeInput::Update(update),
                _ = persistence_tick.tick(), if persistence_dirty => RuntimeInput::Persist,
                _ = activity_tick.tick(), if activity_active => RuntimeInput::ExpireActivities,
            }
        }
        (None, None) => {
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
            // Profiles contain personal data and are fetched on demand. Drop
            // profile fields from previously persisted snapshots.
            state.contacts = Default::default();
            state.profiles.clear();
            state.profile_order.clear();
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
                if let Err(error) = store.save(&state_for_persistence(&state)).await {
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
    match store.save(&state_for_persistence(state)).await {
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
    if let Err(error) = store.save(&state_for_persistence(state)).await {
        let message = error.to_string();
        state.last_error = Some(message.clone());
        let _ = state_tx.send(state.clone());
        let _ = event_tx.send(CoreEvent::Error(message));
    }
}

fn state_for_persistence(state: &CoreState) -> CoreState {
    state.persistence_snapshot()
}
