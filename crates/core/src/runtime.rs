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
    capabilities::CapabilityValue,
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
                    pinned_revision,
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
                            pinned_revision,
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
                    if matches!(&command, CoreCommand::Logout) {
                        events.insert(
                            0,
                            CoreEvent::ConnectionLifecycle {
                                token: result.generation,
                                event: LinkEvent::Logout,
                            },
                        );
                    }
                    attach_pinned_snapshot_revision(&mut events, result.pinned_revision);
                    complete_operation_events(&command, &mut events);
                    let initial_sync = result.sync_required
                        && reduced_state.connection_machine.phase() == LinkPhase::Synchronizing
                        && reduced_state.account.authorization
                            == crate::model::AuthorizationState::Ready;
                    if initial_sync && !has_initial_chat_sync_result(&events) {
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
                RuntimeInput::Command(Some(mut command)) => {
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
                            if let Some(recovered) = recover_interrupted_send(&send, "shutdown") {
                                apply_event(
                                    CoreEvent::MessageSendStateChanged(recovered),
                                    &mut reduced_state,
                                    &state_tx,
                                    &event_tx_task,
                                    &mut activity_tracker,
                                );
                            }
                        }
                        for history in reduced_state.message_history.values_mut() {
                            if history.loading {
                                history.loading = false;
                                history.requested_before_message_id = None;
                                history.error = Some("shutdown interrupted message loading".into());
                            }
                        }
                        reduced_state.chat_pagination.loading = false;
                        reduced_state.chat_pagination.requested_cursor = None;
                        if reduced_state.chat_pagination.loaded {
                            reduced_state.chat_pagination.error =
                                Some("shutdown interrupted chat loading".into());
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
                    let chat_page_request = match &command {
                        CoreCommand::LoadChatsPage { cursor, .. } => Some(cursor.clone()),
                        _ => None,
                    };
                    let search_request = match &command {
                        CoreCommand::SearchMessages(request) => Some(request.clone()),
                        _ => None,
                    };
                    let mut skip_backend = false;
                    if let Some(cursor) = chat_page_request.as_ref() {
                        let pagination = &reduced_state.chat_pagination;
                        if pagination.loading {
                            // Avoid racing two pages requested with the same opaque cursor.
                            skip_backend = true;
                        } else if chat_page_cursor_is_stale(pagination, cursor) {
                            apply_event(
                                CoreEvent::Error(
                                    "chat page cursor is stale; reload the current page".into(),
                                ),
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                                &mut activity_tracker,
                            );
                            skip_backend = true;
                        }
                    }
                    if let CoreCommand::SendMediaReliable {
                        client_id,
                        transfer_generation,
                        session_generation,
                        session_epoch,
                        ..
                    } = &mut command
                    {
                        *transfer_generation = reduced_state
                            .send_ledger
                            .get(client_id)
                            .map_or(1, |send| send.transfer_generation.saturating_add(1));
                        let token = reduced_state.connection_machine.token();
                        *session_generation = token.generation().as_u64();
                        *session_epoch = token.epoch().as_u64();
                    }

                    // Keep the permission boundary in Rust: Compose can make a
                    // control unavailable for usability, but it cannot authorize
                    // a network operation. Unknown snapshots fail closed.
                    if let Some(reason) = command_capability_error(&command, &reduced_state) {
                        for event in
                            operation_error_events(&command, CoreError::InvalidInput(reason))
                        {
                            apply_event(
                                event,
                                &mut reduced_state,
                                &state_tx,
                                &event_tx_task,
                                &mut activity_tracker,
                            );
                        }
                        continue;
                    }

                    if let CoreCommand::SendTextReliable {
                        client_id,
                        chat_id,
                        reply_to_message_id,
                        text,
                    } = &command
                    {
                        if let Some(existing) = reduced_state.send_ledger.get(client_id) {
                            if !existing.same_request(*chat_id, text, *reply_to_message_id) {
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
                                    | OutgoingSendState::Sending
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
                                CoreEvent::MessageSendStateChanged(
                                    OutgoingSend::sending(*client_id, *chat_id, text.clone())
                                        .with_reply_target(*reply_to_message_id),
                                ),
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

                    if let CoreCommand::SendMediaReliable {
                        client_id,
                        chat_id,
                        media,
                        caption,
                        transfer_generation,
                        session_generation,
                        session_epoch,
                    } = &command
                    {
                        if let Some(existing) = reduced_state.send_ledger.get(client_id) {
                            if !existing.same_media_request(*chat_id, media, caption) {
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
                                &existing.state,
                                OutgoingSendState::Sent { .. }
                                    | OutgoingSendState::Rejected { .. }
                                    | OutgoingSendState::Sending
                                    | OutgoingSendState::Uploading { .. }
                                    | OutgoingSendState::Failed {
                                        retryable: false,
                                        ..
                                    }
                            ) {
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
                            // The durable payload, including the private copied path,
                            // is saved before any upload bytes leave the device.
                            apply_event(
                                CoreEvent::MessageSendStateChanged(
                                    OutgoingSend::uploading_media(
                                        *client_id,
                                        *chat_id,
                                        media.clone(),
                                        caption.clone(),
                                        0,
                                        *transfer_generation,
                                    )
                                    .with_session_token(*session_generation, *session_epoch),
                                ),
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
                    if let Some(cursor) = chat_page_request.as_ref() {
                        apply_event(
                            CoreEvent::ChatsPageLoading {
                                cursor: cursor.clone(),
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
                    let sync_required = matches!(
                        &command,
                        CoreCommand::LoadChats { .. }
                            | CoreCommand::LoadChatsPage { cursor: None, .. }
                    ) && reduced_state.connection_machine.phase()
                        == LinkPhase::Synchronizing
                        && reduced_state.account.authorization
                            == crate::model::AuthorizationState::Ready;
                    let pinned_revision = match &command {
                        CoreCommand::LoadPinnedMessages { chat_id } => Some((
                            *chat_id,
                            reduced_state
                                .pinned_message_revision
                                .get(chat_id)
                                .copied()
                                .unwrap_or_default(),
                        )),
                        _ => None,
                    };
                    if worker_tx
                        .try_send(WorkerRequest::Execute {
                            op_id,
                            generation,
                            sync_required,
                            pinned_revision,
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
                                pinned_revision: None,
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
        pinned_revision: Option<(i64, u64)>,
        command: CoreCommand,
    },
}

struct BackendResult {
    op_id: u64,
    generation: crate::connection_machine::ConnectionToken,
    sync_required: bool,
    pinned_revision: Option<(i64, u64)>,
    command: CoreCommand,
    result: Result<Vec<CoreEvent>>,
}

fn operation_error_events(command: &CoreCommand, error: CoreError) -> Vec<CoreEvent> {
    match command {
        CoreCommand::LoadChatsPage { cursor, .. } => vec![CoreEvent::ChatsPageFailed {
            cursor: cursor.clone(),
            error: error.to_string(),
        }],
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
            reply_to_message_id,
            text,
        } => vec![CoreEvent::MessageSendStateChanged(send_error_state(
            *client_id,
            *chat_id,
            text.clone(),
            *reply_to_message_id,
            error,
        ))],
        CoreCommand::SendMediaReliable {
            client_id,
            chat_id,
            media,
            caption,
            transfer_generation,
            session_generation,
            session_epoch,
        } => vec![CoreEvent::MessageSendStateChanged(send_media_error_state(
            *client_id,
            *chat_id,
            media.clone(),
            caption.clone(),
            error,
            *transfer_generation,
            *session_generation,
            *session_epoch,
        ))],
        CoreCommand::PinMessage {
            chat_id,
            message_id,
        } => {
            vec![CoreEvent::PinnedMessageFailed {
                chat_id: *chat_id,
                message_id: *message_id,
                pinned: true,
                error: error.to_string(),
            }]
        }
        CoreCommand::UnpinMessage {
            chat_id,
            message_id,
        } => {
            vec![CoreEvent::PinnedMessageFailed {
                chat_id: *chat_id,
                message_id: *message_id,
                pinned: false,
                error: error.to_string(),
            }]
        }
        CoreCommand::VotePoll { poll_id, .. } => vec![CoreEvent::PollVoteFailed {
            poll_id: *poll_id,
            error: error.to_string(),
        }],
        CoreCommand::AddReaction {
            chat_id,
            message_id,
            ..
        }
        | CoreCommand::RemoveReaction {
            chat_id,
            message_id,
        } => {
            vec![CoreEvent::ReactionOperationFailed {
                chat_id: *chat_id,
                message_id: *message_id,
                error: error.to_string(),
            }]
        }
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
        CoreCommand::EditMessage {
            chat_id,
            message_id,
            ..
        } => vec![CoreEvent::MessageMutationFailed {
            chat_id: *chat_id,
            message_id: *message_id,
            action: crate::event::MessageMutationAction::Edit,
            error: error.to_string(),
        }],
        CoreCommand::DeleteMessageForEveryone {
            chat_id,
            message_id,
        } => vec![CoreEvent::MessageMutationFailed {
            chat_id: *chat_id,
            message_id: *message_id,
            action: crate::event::MessageMutationAction::DeleteForEveryone,
            error: error.to_string(),
        }],
        CoreCommand::DeleteMessageForMe {
            chat_id,
            message_id,
        } => {
            vec![CoreEvent::MessageMutationFailed {
                chat_id: *chat_id,
                message_id: *message_id,
                action: crate::event::MessageMutationAction::DeleteForMe,
                error: error.to_string(),
            }]
        }
        _ => vec![CoreEvent::Error(error.to_string())],
    }
}

fn has_initial_chat_sync_result(events: &[CoreEvent]) -> bool {
    events.iter().any(|event| {
        matches!(event, CoreEvent::ChatsLoaded(_))
            || matches!(event, CoreEvent::ChatsPageLoaded(page) if page.requested_cursor.is_none())
    })
}

fn chat_page_cursor_is_stale(
    pagination: &crate::chat_pagination::ChatPaginationState,
    cursor: &Option<String>,
) -> bool {
    if pagination.loaded {
        cursor
            .as_ref()
            .is_some_and(|cursor| pagination.next_cursor.as_ref() != Some(cursor))
    } else {
        cursor.is_some()
    }
}

fn command_capability_error(command: &CoreCommand, state: &CoreState) -> Option<String> {
    // Unauthenticated requests must retain the backend's authorization error;
    // no session-bound rights exist to consult until authorization completes.
    if state.account.authorization != crate::model::AuthorizationState::Ready {
        return None;
    }
    let require_send = |chat_id: i64| match state.chat_capabilities.get(&chat_id) {
        Some(capabilities) if capabilities.can_send_now() == CapabilityValue::Known(true) => None,
        Some(capabilities) if capabilities.can_send_now() == CapabilityValue::Known(false) => {
            Some("Telegram currently does not allow sending in this chat".to_owned())
        }
        _ => Some("Telegram send permission is unavailable; refresh chat permissions".to_owned()),
    };
    match command {
        CoreCommand::SendText { chat_id, .. } | CoreCommand::SendTextReliable { chat_id, .. } => {
            require_send(*chat_id)
        }
        CoreCommand::SendMediaReliable { chat_id, .. } => {
            if let Some(reason) = require_send(*chat_id) {
                return Some(reason);
            }
            if state.chat_capabilities.get(chat_id).map_or(true, |cap| {
                cap.can_send_media != CapabilityValue::Known(true)
            }) {
                Some("Telegram does not confirm media sending is allowed in this chat".into())
            } else {
                None
            }
        }
        CoreCommand::SendLocation { chat_id, .. } => {
            if let Some(reason) = require_send(*chat_id) {
                return Some(reason);
            }
            if state.chat_capabilities.get(chat_id).map_or(true, |cap| {
                cap.can_send_media != CapabilityValue::Known(true)
            }) {
                Some("Telegram does not confirm media sending is allowed in this chat".into())
            } else {
                None
            }
        }
        CoreCommand::ForwardMessages(request) => {
            let source = state.chat_capabilities.get(&request.source_chat_id);
            if source.map_or(true, |cap| {
                cap.can_forward_messages != CapabilityValue::Known(true)
            }) {
                return Some(
                    "Telegram does not confirm forwarding is allowed from this chat".into(),
                );
            }
            if let Some(reason) = require_send(request.destination_chat_id) {
                return Some(reason);
            }
            let forwards_media_or_unknown = request.message_ids.iter().any(|id| {
                state
                    .messages
                    .get(&request.source_chat_id)
                    .and_then(|messages| messages.iter().find(|message| message.id == *id))
                    .map_or(true, |message| !message.attachments.is_empty())
            });
            if forwards_media_or_unknown
                && state
                    .chat_capabilities
                    .get(&request.destination_chat_id)
                    .map_or(true, |cap| {
                        cap.can_send_media != CapabilityValue::Known(true)
                    })
            {
                return Some(
                    "Telegram does not confirm media sending is allowed in the destination chat"
                        .into(),
                );
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;
    use crate::{capabilities::ChatCapabilities, command::CoreCommand};

    #[test]
    fn only_a_successful_first_chat_page_completes_initial_sync() {
        let first_page = CoreEvent::ChatsPageLoaded(crate::chat_pagination::ChatPage {
            requested_cursor: None,
            chats: Vec::new(),
            next_cursor: Some("peer-1".into()),
        });
        assert!(has_initial_chat_sync_result(&[first_page]));

        let later_page = CoreEvent::ChatsPageLoaded(crate::chat_pagination::ChatPage {
            requested_cursor: Some("peer-1".into()),
            chats: Vec::new(),
            next_cursor: None,
        });
        assert!(!has_initial_chat_sync_result(&[later_page]));
        assert!(!has_initial_chat_sync_result(&[
            CoreEvent::ChatsPageFailed {
                cursor: None,
                error: "network down".into(),
            }
        ]));
    }

    #[test]
    fn pagination_accepts_first_page_refresh_and_rejects_stale_continuation() {
        let pagination = crate::chat_pagination::ChatPaginationState {
            loaded: true,
            next_cursor: Some("current".into()),
            ..Default::default()
        };
        assert!(!chat_page_cursor_is_stale(&pagination, &None));
        assert!(!chat_page_cursor_is_stale(
            &pagination,
            &Some("current".into())
        ));
        assert!(chat_page_cursor_is_stale(
            &pagination,
            &Some("expired".into())
        ));
    }

    fn allowed(chat_id: i64) -> ChatCapabilities {
        let mut capabilities = ChatCapabilities {
            chat_id,
            can_send_messages: CapabilityValue::Known(true),
            can_forward_messages: CapabilityValue::Known(true),
            can_send_media: CapabilityValue::Known(true),
            slow_mode_enabled: CapabilityValue::Known(false),
            slow_mode_remaining_seconds: CapabilityValue::Known(None),
            ..ChatCapabilities::default()
        };
        capabilities.refresh_derived();
        capabilities
    }

    #[test]
    fn sending_requires_an_authoritative_current_permission() {
        let command = CoreCommand::SendText {
            chat_id: 8,
            text: "hello".into(),
        };
        let mut state = CoreState::default();
        state.account.authorization = crate::model::AuthorizationState::Ready;
        assert!(command_capability_error(&command, &state).is_some());
        state.chat_capabilities.insert(8, allowed(8));
        assert_eq!(command_capability_error(&command, &state), None);
    }

    #[test]
    fn forwarding_requires_source_and_destination_permissions() {
        let command = CoreCommand::ForwardMessages(crate::forwarding::ForwardMessagesRequest {
            request_id: "forward-1".into(),
            source_chat_id: 2,
            message_ids: vec![5],
            destination_chat_id: 3,
        });
        let mut state = CoreState::default();
        state.account.authorization = crate::model::AuthorizationState::Ready;
        state.chat_capabilities.insert(2, allowed(2));
        assert!(command_capability_error(&command, &state).is_some());
        state.chat_capabilities.insert(3, allowed(3));
        assert_eq!(command_capability_error(&command, &state), None);
        let mut no_media = allowed(3);
        no_media.can_send_media = CapabilityValue::Known(false);
        state.chat_capabilities.insert(3, no_media);
        assert!(command_capability_error(&command, &state).is_some());
    }
}

fn attach_pinned_snapshot_revision(events: &mut [CoreEvent], revision: Option<(i64, u64)>) {
    let Some((requested_chat_id, requested_revision)) = revision else {
        return;
    };
    for event in events {
        if let CoreEvent::PinnedMessagesLoaded {
            chat_id,
            request_revision,
            ..
        } = event
        {
            if *chat_id == requested_chat_id {
                *request_revision = Some(requested_revision);
            }
        }
    }
}

fn complete_operation_events(command: &CoreCommand, events: &mut Vec<CoreEvent>) {
    if let CoreCommand::LoadChatsPage { cursor, .. } = command {
        if !events.iter().any(|event| matches!(event,
            CoreEvent::ChatsPageLoaded(page) if &page.requested_cursor == cursor
        ) || matches!(event, CoreEvent::ChatsPageFailed { cursor: event_cursor, .. } if event_cursor == cursor)) {
            events.push(CoreEvent::ChatsPageFailed {
                cursor: cursor.clone(),
                error: "backend returned no chat page completion".into(),
            });
        }
    }
    if let CoreCommand::PinMessage {
        chat_id,
        message_id,
    }
    | CoreCommand::UnpinMessage {
        chat_id,
        message_id,
    } = command
    {
        let pinned = matches!(command, CoreCommand::PinMessage { .. });
        let confirmed = events.iter().any(|event| {
            matches!(
                event,
                CoreEvent::PinnedMessagesChanged {
                    chat_id: event_chat,
                    message_ids,
                    pinned: event_pinned,
                    ..
                } if event_chat == chat_id
                    && message_ids.contains(message_id)
                    && event_pinned == &pinned
            ) || matches!(
                event,
                CoreEvent::PinnedMessageFailed {
                    chat_id: event_chat,
                    message_id: event_message,
                    ..
                } if event_chat == chat_id && event_message == message_id
            )
        });
        if !confirmed {
            events.push(CoreEvent::PinnedMessageFailed {
                chat_id: *chat_id,
                message_id: *message_id,
                pinned,
                error: "backend returned without confirming the pin change".into(),
            });
        }
    }
    if let CoreCommand::EditMessage {
        chat_id,
        message_id,
        ..
    }
    | CoreCommand::DeleteMessageForMe {
        chat_id,
        message_id,
    }
    | CoreCommand::DeleteMessageForEveryone {
        chat_id,
        message_id,
    } = command
    {
        let expected_action = match command {
            CoreCommand::EditMessage { .. } => crate::event::MessageMutationAction::Edit,
            CoreCommand::DeleteMessageForMe { .. } => {
                crate::event::MessageMutationAction::DeleteForMe
            }
            CoreCommand::DeleteMessageForEveryone { .. } => {
                crate::event::MessageMutationAction::DeleteForEveryone
            }
            _ => unreachable!(),
        };
        let confirmed = events.iter().any(|event| {
            matches!(
                event,
                CoreEvent::MessageMutationSucceeded {
                    chat_id: event_chat,
                    message_id: event_message,
                    action,
                } if event_chat == chat_id && event_message == message_id && action == &expected_action
            ) || matches!(
                event,
                CoreEvent::MessageMutationFailed {
                    chat_id: event_chat,
                    message_id: event_message,
                    action,
                    ..
                } if event_chat == chat_id && event_message == message_id && action == &expected_action
            )
        });
        if !confirmed {
            events.push(CoreEvent::MessageMutationFailed {
                chat_id: *chat_id,
                message_id: *message_id,
                action: expected_action,
                error: "backend returned without confirming the message change".into(),
            });
        }
    }
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
        reply_to_message_id,
        text,
    } = command
    {
        if !events.iter().any(|event| matches!(event, CoreEvent::MessageSendStateChanged(send) if send.client_id == *client_id && send.same_request(*chat_id, text, *reply_to_message_id) && !matches!(send.state, OutgoingSendState::Sending))) {
            events.push(CoreEvent::MessageSendStateChanged(OutgoingSend::awaiting_confirmation(*client_id, *chat_id, text.clone(), "backend returned without confirming the send outcome").with_reply_target(*reply_to_message_id)));
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
        CoreCommand::LoadChatsPage { cursor, .. } => apply_event(
            CoreEvent::ChatsPageFailed {
                cursor: cursor.clone(),
                error: "connection generation changed while loading chats".into(),
            },
            state,
            state_tx,
            event_tx,
            activity_tracker,
        ),
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
            reply_to_message_id,
            text,
        } => apply_event(
            CoreEvent::MessageSendStateChanged(
                OutgoingSend::awaiting_confirmation(
                    *client_id,
                    *chat_id,
                    text.clone(),
                    "connection changed while send outcome was unknown",
                )
                .with_reply_target(*reply_to_message_id),
            ),
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

fn recover_interrupted_send(send: &OutgoingSend, cause: &str) -> Option<OutgoingSend> {
    match (&send.state, send.media.clone()) {
        (OutgoingSendState::Uploading { .. }, Some(media)) => Some(
            OutgoingSend::failed_media(
                send.client_id,
                send.chat_id,
                media,
                send.text.clone(),
                format!("{cause} interrupted the media upload"),
                true,
                send.transfer_generation,
            )
            .with_session_token(send.session_generation, send.session_epoch),
        ),
        (OutgoingSendState::Sending, Some(media)) => Some(
            OutgoingSend::failed_media(
                send.client_id,
                send.chat_id,
                media,
                send.text.clone(),
                format!(
                    "{cause} interrupted the media send; delivery is unknown. Retry with the same Telegram send ID to reconcile it"
                ),
                true,
                send.transfer_generation,
            )
            .with_session_token(send.session_generation, send.session_epoch),
        ),
        (OutgoingSendState::Sending, None) => Some(
            OutgoingSend::awaiting_confirmation(
                send.client_id,
                send.chat_id,
                send.text.clone(),
                format!("{cause} interrupted the send; delivery is unknown"),
            )
            .with_session_token(send.session_generation, send.session_epoch),
        ),
        _ => None,
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
            normalize_restored_media_downloads(&mut state);
            for history in state.message_history.values_mut() {
                history.loading = false;
                history.requested_before_message_id = None;
            }
            state.chat_pagination.loading = false;
            state.chat_pagination.requested_cursor = None;
            for (client_id, send) in state.outbox.clone() {
                state.send_ledger.entry(client_id).or_insert(send);
            }
            let recovering: Vec<_> = state
                .outbox
                .values()
                .filter(|send| {
                    matches!(
                        send.state,
                        OutgoingSendState::Sending | OutgoingSendState::Uploading { .. }
                    )
                })
                .map(|send| send.client_id)
                .collect();
            let mut recovered_any = false;
            for client_id in recovering {
                if let Some(send) = state.outbox.get(&client_id).cloned() {
                    let Some(recovered) = recover_interrupted_send(&send, "process restart") else {
                        continue;
                    };
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
            state.restore_complete = true;
            let _ = state_tx.send(state.clone());
            state
        }
        Ok(None) => {
            let mut state = CoreState::default();
            state.restore_complete = true;
            let _ = state_tx.send(state.clone());
            state
        }
        Err(error) => {
            let mut state = CoreState::default();
            let message = error.to_string();
            state.last_error = Some(message.clone());
            state.restore_complete = true;
            let _ = state_tx.send(state.clone());
            let _ = event_tx.send(CoreEvent::Error(message));
            state
        }
    }
}

fn normalize_restored_media_downloads(state: &mut CoreState) {
    fn normalize_message(message: &mut crate::model::MessageSummary) {
        for attachment in &mut message.attachments {
            use crate::model::AttachmentDownloadState as DownloadState;

            match &attachment.download_state {
                DownloadState::Queued | DownloadState::Downloading { .. } => {
                    attachment.download_state = DownloadState::NotDownloaded;
                    attachment.download_generation = 0;
                }
                // TransferManager is process-local, so every persisted generation
                // is stale after restart. Keep terminal UI state (including a
                // failure reason and downloaded path) but reset its generation so
                // fresh callbacks and retries are accepted.
                _ if attachment.download_generation > 0 => {
                    attachment.download_generation = 0;
                }
                _ => {}
            }
        }
    }

    for messages in state.messages.values_mut() {
        for message in messages {
            normalize_message(message);
        }
    }
    for chat in &mut state.account.chats {
        if let Some(message) = &mut chat.last_message {
            normalize_message(message);
        }
    }
}

fn send_media_error_state(
    client_id: i64,
    chat_id: i64,
    media: crate::send::OutgoingMediaAttachment,
    caption: String,
    error: CoreError,
    transfer_generation: u64,
    session_generation: u64,
    session_epoch: u64,
) -> OutgoingSend {
    let send = match error {
        CoreError::SendOutcomeUnknown(reason) => OutgoingSend::awaiting_confirmation_media(
            client_id,
            chat_id,
            media,
            caption,
            reason,
            transfer_generation,
        ),
        CoreError::FloodWait { seconds } => OutgoingSend::failed_media(
            client_id,
            chat_id,
            media,
            caption,
            format!("Telegram rate limit: wait {seconds} seconds"),
            true,
            transfer_generation,
        ),
        CoreError::TransientTransport(reason) | CoreError::TransientSend(reason) => {
            OutgoingSend::failed_media(
                client_id,
                chat_id,
                media,
                caption,
                reason,
                true,
                transfer_generation,
            )
        }
        CoreError::SendRejected(reason) => OutgoingSend::failed_media(
            client_id,
            chat_id,
            media,
            caption,
            reason,
            false,
            transfer_generation,
        ),
        other => OutgoingSend::failed_media(
            client_id,
            chat_id,
            media,
            caption,
            other.to_string(),
            false,
            transfer_generation,
        ),
    };
    send.with_session_token(session_generation, session_epoch)
}

fn send_error_state(
    client_id: i64,
    chat_id: i64,
    text: String,
    reply_to_message_id: Option<i64>,
    error: CoreError,
) -> OutgoingSend {
    let send = match error {
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
    };
    send.with_reply_target(reply_to_message_id)
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
    // Capability snapshots are session-only, but still must reach CoreState so
    // Rust command authorization and Android controls see Telegram's rights.
    if !event.is_transient() || matches!(event, CoreEvent::ChatCapabilitiesUpdated(_)) {
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

#[cfg(test)]
mod pin_failure_tests {
    use super::{complete_operation_events, operation_error_events};
    use crate::{command::CoreCommand, error::CoreError, event::CoreEvent};

    #[test]
    fn pin_and_unpin_errors_are_bound_to_the_requested_message() {
        for (command, expected_pinned) in [
            (
                CoreCommand::PinMessage {
                    chat_id: -20,
                    message_id: 7,
                },
                true,
            ),
            (
                CoreCommand::UnpinMessage {
                    chat_id: -20,
                    message_id: 7,
                },
                false,
            ),
        ] {
            let events = operation_error_events(
                &command,
                CoreError::SendRejected("permission denied".into()),
            );
            assert_eq!(
                events,
                vec![CoreEvent::PinnedMessageFailed {
                    chat_id: -20,
                    message_id: 7,
                    pinned: expected_pinned,
                    error: "Telegram rejected the outgoing message: permission denied".into(),
                }]
            );
        }
    }

    #[test]
    fn empty_pin_backend_result_becomes_a_targeted_failure() {
        let mut events = Vec::new();
        complete_operation_events(
            &CoreCommand::UnpinMessage {
                chat_id: -20,
                message_id: 7,
            },
            &mut events,
        );
        assert_eq!(
            events,
            vec![CoreEvent::PinnedMessageFailed {
                chat_id: -20,
                message_id: 7,
                pinned: false,
                error: "backend returned without confirming the pin change".into(),
            }]
        );
    }
}

#[cfg(test)]
mod interrupted_send_recovery_tests {
    use super::recover_interrupted_send;
    use crate::send::{
        OutgoingMediaAttachment, OutgoingMediaKind, OutgoingSend, OutgoingSendState,
    };

    fn video_media() -> OutgoingMediaAttachment {
        OutgoingMediaAttachment {
            kind: OutgoingMediaKind::Video,
            path: "/private/outgoing/video.mp4".into(),
            file_name: "video.mp4".into(),
            mime_type: "video/mp4".into(),
            size_bytes: 4096,
            voice_duration_ms: None,
            video_duration_ms: Some(1250),
            video_width: Some(640),
            video_height: Some(360),
        }
    }

    #[test]
    fn interrupted_media_send_becomes_retryable_and_keeps_the_same_id() {
        let send = OutgoingSend::sending_media(41, 7, video_media(), "caption", 3)
            .with_session_token(8, 12);

        let recovered = recover_interrupted_send(&send, "process restart").unwrap();

        assert_eq!(recovered.client_id, 41);
        assert_eq!(recovered.chat_id, 7);
        assert_eq!(recovered.media, send.media);
        assert_eq!(recovered.transfer_generation, 3);
        assert_eq!(recovered.session_generation, 8);
        assert_eq!(recovered.session_epoch, 12);
        assert!(matches!(
            recovered.state,
            OutgoingSendState::Failed {
                retryable: true,
                ..
            }
        ));
    }

    #[test]
    fn interrupted_upload_keeps_a_retryable_private_file_reference() {
        let send = OutgoingSend::uploading_media(42, 7, video_media(), "caption", 1024, 4)
            .with_session_token(8, 12);

        let recovered = recover_interrupted_send(&send, "shutdown").unwrap();

        assert_eq!(recovered.media, send.media);
        assert_eq!(recovered.transfer_generation, 4);
        assert!(matches!(
            recovered.state,
            OutgoingSendState::Failed {
                retryable: true,
                ..
            }
        ));
    }

    #[test]
    fn interrupted_text_send_still_requires_confirmation() {
        let send = OutgoingSend::sending(43, 7, "hello").with_session_token(8, 12);

        let recovered = recover_interrupted_send(&send, "process restart").unwrap();

        assert!(matches!(
            recovered.state,
            OutgoingSendState::AwaitingConfirmation { .. }
        ));
        assert_eq!(recovered.session_generation, 8);
        assert_eq!(recovered.session_epoch, 12);
    }
}

fn state_for_persistence(state: &CoreState) -> CoreState {
    state.persistence_snapshot()
}

#[cfg(test)]
mod message_mutation_completion_tests {
    use super::complete_operation_events;
    use crate::{
        command::CoreCommand,
        event::{CoreEvent, MessageMutationAction},
    };

    #[test]
    fn empty_mutation_backend_result_becomes_a_targeted_failure() {
        let commands = [
            (
                CoreCommand::EditMessage {
                    chat_id: 17,
                    message_id: 42,
                    text: "updated".into(),
                },
                MessageMutationAction::Edit,
            ),
            (
                CoreCommand::DeleteMessageForMe {
                    chat_id: 17,
                    message_id: 42,
                },
                MessageMutationAction::DeleteForMe,
            ),
            (
                CoreCommand::DeleteMessageForEveryone {
                    chat_id: 17,
                    message_id: 42,
                },
                MessageMutationAction::DeleteForEveryone,
            ),
        ];

        for (command, action) in commands {
            let mut events = Vec::new();
            complete_operation_events(&command, &mut events);
            assert_eq!(
                events,
                vec![CoreEvent::MessageMutationFailed {
                    chat_id: 17,
                    message_id: 42,
                    action,
                    error: "backend returned without confirming the message change".into(),
                }]
            );
        }
    }
}

#[cfg(test)]
mod media_restore_generation_tests {
    use super::*;
    use crate::model::{
        AccountState, AttachmentDownloadState, AttachmentKind, ChatSummary, MediaAttachment,
        MessageSummary,
    };

    fn message() -> MessageSummary {
        message_with_download(
            AttachmentDownloadState::Downloading {
                bytes_transferred: 128,
                total_bytes: Some(512),
            },
            7,
        )
    }

    fn message_with_download(
        download_state: AttachmentDownloadState,
        download_generation: u64,
    ) -> MessageSummary {
        MessageSummary {
            id: 23,
            chat_id: 7,
            sender_id: 9,
            text: String::new(),
            timestamp_unix: 1,
            outgoing: false,
            location: None,
            attachments: vec![MediaAttachment {
                kind: AttachmentKind::Photo,
                file_reference: "opaque-reference".into(),
                size_bytes: None,
                mime_type: None,
                width: None,
                height: None,
                duration_ms: None,
                voice_waveform: None,
                filename: None,
                thumbnail: None,
                download_generation,
                download_state,
            }],
            forwarded_from: None,
            edited: false,
            edit_timestamp_unix: None,
            service_message: false,
            service: None,
            media_caption: None,
            reply_to: None,
            reactions: Vec::new(),
            reaction_revision: None,
        }
    }

    #[tokio::test]
    async fn restored_active_download_accepts_generation_one_update() {
        let store = crate::storage::MemoryStateStore::default();
        let mut persisted = CoreState::default();
        let last_message = message();
        persisted.messages.insert(7, vec![last_message.clone()]);
        persisted.account = AccountState {
            authorization: crate::model::AuthorizationState::Ready,
            chats: vec![ChatSummary {
                id: 7,
                title: "chat".into(),
                unread_count: 0,
                archived: None,
                muted: None,
                pinned: None,
                is_unread: None,
                unread_mentions_count: None,
                unread_reactions_count: None,
                last_message: Some(last_message),
            }],
        };
        store.save(&persisted).await.unwrap();
        let (state_tx, _) = watch::channel(CoreState::default());
        let (event_tx, _) = broadcast::channel(8);
        let mut state = restore_state(&store, &state_tx, &event_tx).await;

        let (state_tx, _) = watch::channel(state.clone());
        let (event_tx, _) = broadcast::channel(8);
        apply_event(
            CoreEvent::MediaTransferUpdated {
                chat_id: 7,
                message_id: 23,
                attachment_index: 0,
                generation: 1,
                state: AttachmentDownloadState::Queued,
            },
            &mut state,
            &state_tx,
            &event_tx,
            &mut ChatActivityTracker::default(),
        );

        let cached = &state.messages[&7][0].attachments[0];
        assert_eq!(cached.download_generation, 1);
        assert_eq!(cached.download_state, AttachmentDownloadState::Queued);
        let last = state.account.chats[0].last_message.as_ref().unwrap();
        assert_eq!(last.attachments[0].download_generation, 1);
        assert_eq!(
            last.attachments[0].download_state,
            AttachmentDownloadState::Queued
        );
    }

    #[tokio::test]
    async fn restored_terminal_downloads_keep_visible_state_and_accept_fresh_update() {
        for (persisted_state, expected_state) in [
            (
                AttachmentDownloadState::Failed {
                    reason: Some("network interrupted".into()),
                },
                AttachmentDownloadState::Failed {
                    reason: Some("network interrupted".into()),
                },
            ),
            (
                AttachmentDownloadState::Cancelled,
                AttachmentDownloadState::Cancelled,
            ),
            (
                AttachmentDownloadState::Downloaded {
                    local_path: std::path::PathBuf::from("/cache/attachment"),
                },
                AttachmentDownloadState::Downloaded {
                    local_path: std::path::PathBuf::from("/cache/attachment"),
                },
            ),
        ] {
            let store = crate::storage::MemoryStateStore::default();
            let persisted_message = message_with_download(persisted_state, 3);
            let mut persisted = CoreState::default();
            persisted
                .messages
                .insert(7, vec![persisted_message.clone()]);
            persisted.account = AccountState {
                authorization: crate::model::AuthorizationState::Ready,
                chats: vec![ChatSummary {
                    id: 7,
                    title: "chat".into(),
                    unread_count: 0,
                    archived: None,
                    muted: None,
                    pinned: None,
                    is_unread: None,
                    unread_mentions_count: None,
                    unread_reactions_count: None,
                    last_message: Some(persisted_message),
                }],
            };
            store.save(&persisted).await.unwrap();

            let (state_tx, _) = watch::channel(CoreState::default());
            let (event_tx, _) = broadcast::channel(8);
            let mut state = restore_state(&store, &state_tx, &event_tx).await;

            let cached = &state.messages[&7][0].attachments[0];
            assert_eq!(cached.download_generation, 0);
            assert_eq!(cached.download_state, expected_state);
            let last = state.account.chats[0].last_message.as_ref().unwrap();
            assert_eq!(last.attachments[0].download_generation, 0);
            assert_eq!(last.attachments[0].download_state, expected_state);

            let (state_tx, _) = watch::channel(state.clone());
            let (event_tx, _) = broadcast::channel(8);
            apply_event(
                CoreEvent::MediaTransferUpdated {
                    chat_id: 7,
                    message_id: 23,
                    attachment_index: 0,
                    generation: 1,
                    state: AttachmentDownloadState::Queued,
                },
                &mut state,
                &state_tx,
                &event_tx,
                &mut ChatActivityTracker::default(),
            );

            assert_eq!(state.messages[&7][0].attachments[0].download_generation, 1);
            assert_eq!(
                state.messages[&7][0].attachments[0].download_state,
                AttachmentDownloadState::Queued
            );
            let last = state.account.chats[0].last_message.as_ref().unwrap();
            assert_eq!(last.attachments[0].download_generation, 1);
            assert_eq!(
                last.attachments[0].download_state,
                AttachmentDownloadState::Queued
            );
        }
    }
}
