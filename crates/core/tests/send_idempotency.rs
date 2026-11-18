use std::sync::{Arc, Mutex};

use tg_mobile_core::{
    backend_update_channel, decode_command, spawn_core_with_store, BackendFuture,
    BackendUpdateReceiver, BackendUpdateSender, CoreCommand, CoreError, CoreEvent, CoreState,
    LinkEvent, MemoryStateStore, MessageSummary, MockTelegramBackend, OutgoingMediaAttachment,
    OutgoingMediaKind, OutgoingSend, OutgoingSendState, StateStore, TelegramBackend,
};
use tokio::time::{timeout, Duration};

async fn authorize(backend: &mut MockTelegramBackend) {
    backend.handle(CoreCommand::Start).await.unwrap();
    backend
        .handle(CoreCommand::SetPhoneNumber("+10000000000".into()))
        .await
        .unwrap();
    backend
        .handle(CoreCommand::SubmitCode("12345".into()))
        .await
        .unwrap();
}

#[test]
fn media_send_completion_survives_reconnect_but_not_retry_or_account_replacement() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ConnectionSessionStarted);
    for event in [
        LinkEvent::TransportConnected,
        LinkEvent::SyncStarted,
        LinkEvent::SyncCompleted,
    ] {
        let token = state.connection_machine.token();
        state.reduce(&CoreEvent::ConnectionLifecycle { token, event });
    }
    let first_token = state.connection_machine.token();
    let media = OutgoingMediaAttachment {
        kind: OutgoingMediaKind::Voice,
        path: "/private/voice.ogg".into(),
        file_name: "voice.ogg".into(),
        mime_type: "audio/ogg".into(),
        size_bytes: 100,
        voice_duration_ms: Some(1_000),
        video_duration_ms: None,
        video_width: None,
        video_height: None,
    };

    let initial_upload = OutgoingSend::uploading_media(700, 42, media.clone(), "", 0, 1)
        .with_session_token(
            first_token.generation().as_u64(),
            first_token.epoch().as_u64(),
        );
    state.reduce(&CoreEvent::MessageSendStateChanged(initial_upload));

    state.reduce(&CoreEvent::ConnectionLifecycle {
        token: first_token,
        event: LinkEvent::TransportLost,
    });
    let reconnected_token = state.connection_machine.token();
    assert_eq!(reconnected_token.generation(), first_token.generation());
    assert!(reconnected_token.epoch() > first_token.epoch());

    // A callback from the same account and same upload attempt may arrive after
    // reconnect; accepting it prevents the durable outbox from remaining stuck
    // at its last pre-disconnect progress value.
    let reconnect_progress = OutgoingSend::uploading_media(700, 42, media.clone(), "", 25, 1)
        .with_session_token(
            first_token.generation().as_u64(),
            first_token.epoch().as_u64(),
        );
    state.reduce(&CoreEvent::MessageSendStateChanged(reconnect_progress));
    assert!(matches!(
        state.outbox.get(&700).map(|send| &send.state),
        Some(OutgoingSendState::Uploading {
            bytes_transferred: 25,
            ..
        })
    ));

    // Retrying creates a newer attempt, so callbacks from the old attempt stay
    // stale even though same-session callbacks may cross connection epochs.
    let retried_upload = OutgoingSend::uploading_media(700, 42, media.clone(), "", 0, 2)
        .with_session_token(
            reconnected_token.generation().as_u64(),
            reconnected_token.epoch().as_u64(),
        );
    state.reduce(&CoreEvent::MessageSendStateChanged(retried_upload));
    let stale_attempt = OutgoingSend::uploading_media(700, 42, media.clone(), "", 50, 1)
        .with_session_token(
            first_token.generation().as_u64(),
            first_token.epoch().as_u64(),
        );
    state.reduce(&CoreEvent::MessageSendStateChanged(stale_attempt));
    assert_eq!(state.outbox.get(&700).unwrap().transfer_generation, 2);
    assert_eq!(state.outbox.get(&700).unwrap().bytes_transferred, 0);

    let sent = OutgoingSend::sent_media(700, 42, media.clone(), "", 800, 2).with_session_token(
        reconnected_token.generation().as_u64(),
        first_token.epoch().as_u64(),
    );
    state.reduce(&CoreEvent::MessageSendStateChanged(sent));
    assert!(!state.outbox.contains_key(&700));

    state.reduce(&CoreEvent::ConnectionLifecycle {
        token: reconnected_token,
        event: LinkEvent::Logout,
    });
    state.reduce(&CoreEvent::ConnectionSessionStarted);
    let replacement_token = state.connection_machine.token();
    let replacement_upload = OutgoingSend::uploading_media(700, 42, media.clone(), "", 0, 1)
        .with_session_token(
            replacement_token.generation().as_u64(),
            replacement_token.epoch().as_u64(),
        );
    state.reduce(&CoreEvent::MessageSendStateChanged(replacement_upload));
    let old_account_completion = OutgoingSend::sent_media(700, 42, media, "", 800, 1)
        .with_session_token(
            first_token.generation().as_u64(),
            first_token.epoch().as_u64(),
        );
    state.reduce(&CoreEvent::MessageSendStateChanged(old_account_completion));
    assert!(matches!(
        state.outbox.get(&700).map(|send| &send.state),
        Some(OutgoingSendState::Uploading { .. })
    ));
}

#[tokio::test]
async fn retrying_same_client_id_does_not_add_a_second_message() {
    let mut backend = MockTelegramBackend::default();
    authorize(&mut backend).await;

    let command = CoreCommand::SendTextReliable {
        client_id: 991,
        chat_id: 2,

        reply_to_message_id: None,
        text: "hello".into(),
    };

    let first = backend.handle(command.clone()).await.unwrap();
    let first_message_id = first
        .iter()
        .find_map(|event| match event {
            CoreEvent::MessageAdded(message) => Some(message.id),
            _ => None,
        })
        .expect("first send must add the message");

    assert!(first.iter().any(|event| matches!(
        event,
        CoreEvent::MessageSendStateChanged(OutgoingSend {
            state: OutgoingSendState::Sent { message_id },
            ..
        }) if *message_id == first_message_id
    )));

    let retry = backend.handle(command).await.unwrap();
    assert!(
        !retry
            .iter()
            .any(|event| matches!(event, CoreEvent::MessageAdded(_))),
        "an idempotent retry must not add a duplicate message"
    );
    assert!(retry.iter().any(|event| matches!(
        event,
        CoreEvent::MessageSendStateChanged(OutgoingSend {
            state: OutgoingSendState::Sent { message_id },
            ..
        }) if *message_id == first_message_id
    )));
}

#[tokio::test]
async fn same_text_with_different_client_ids_creates_two_messages() {
    let mut backend = MockTelegramBackend::default();
    authorize(&mut backend).await;

    let first = backend
        .handle(CoreCommand::SendTextReliable {
            client_id: 501,
            chat_id: 2,

            reply_to_message_id: None,
            text: "same text".into(),
        })
        .await
        .unwrap();
    let second = backend
        .handle(CoreCommand::SendTextReliable {
            client_id: 502,
            chat_id: 2,

            reply_to_message_id: None,
            text: "same text".into(),
        })
        .await
        .unwrap();

    let first_id = first.iter().find_map(|event| match event {
        CoreEvent::MessageAdded(message) => Some(message.id),
        _ => None,
    });
    let second_id = second.iter().find_map(|event| match event {
        CoreEvent::MessageAdded(message) => Some(message.id),
        _ => None,
    });
    assert!(first_id.is_some());
    assert!(second_id.is_some());
    assert_ne!(first_id, second_id);
}

#[tokio::test]
async fn reliable_reply_is_idempotent_and_reply_target_is_part_of_the_payload() {
    let mut backend = MockTelegramBackend::default();
    authorize(&mut backend).await;
    let reply = CoreCommand::SendTextReliable {
        client_id: 503,
        chat_id: 2,
        reply_to_message_id: Some(41),
        text: "reply once".into(),
    };

    let first = backend.handle(reply.clone()).await.unwrap();
    let message_id = first
        .iter()
        .find_map(|event| match event {
            CoreEvent::MessageAdded(message) => {
                assert_eq!(
                    message.reply_to.as_ref().map(|reply| reply.message_id),
                    Some(41)
                );
                Some(message.id)
            }
            _ => None,
        })
        .expect("reply should create one message");
    let retry = backend.handle(reply).await.unwrap();
    assert!(!retry
        .iter()
        .any(|event| matches!(event, CoreEvent::MessageAdded(_))));
    assert!(retry.iter().any(|event| matches!(event,
        CoreEvent::MessageSendStateChanged(send)
            if matches!(&send.state, OutgoingSendState::Sent { message_id: id } if *id == message_id)
    )));

    let collision = backend
        .handle(CoreCommand::SendTextReliable {
            client_id: 503,
            chat_id: 2,
            reply_to_message_id: Some(42),
            text: "reply once".into(),
        })
        .await
        .unwrap();
    assert!(!collision
        .iter()
        .any(|event| matches!(event, CoreEvent::MessageAdded(_))));
    assert!(collision.iter().any(|event| matches!(event,
        CoreEvent::MessageSendStateChanged(send)
            if matches!(&send.state, OutgoingSendState::Failed { retryable: false, .. })
    )));
}

#[tokio::test]
async fn reusing_client_id_for_different_content_is_rejected_without_sending() {
    let mut backend = MockTelegramBackend::default();
    authorize(&mut backend).await;

    backend
        .handle(CoreCommand::SendTextReliable {
            client_id: 7,
            chat_id: 2,

            reply_to_message_id: None,
            text: "first".into(),
        })
        .await
        .unwrap();

    let events = backend
        .handle(CoreCommand::SendTextReliable {
            client_id: 7,
            chat_id: 2,

            reply_to_message_id: None,
            text: "different".into(),
        })
        .await
        .unwrap();

    assert!(!events
        .iter()
        .any(|event| matches!(event, CoreEvent::MessageAdded(_))));
    assert!(events.iter().any(|event| matches!(
        event,
        CoreEvent::MessageSendStateChanged(OutgoingSend {
            state: OutgoingSendState::Failed {
                retryable: false,
                ..
            },
            ..
        })
    )));
}

#[test]
fn core_state_tracks_retryable_outbox_and_removes_success() {
    let mut state = CoreState::default();

    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
        101, 2, "hello",
    )));
    assert!(matches!(
        state.outgoing_send(101).map(|send| &send.state),
        Some(OutgoingSendState::Sending)
    ));

    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::failed(
        101,
        2,
        "hello",
        "network unavailable",
        true,
    )));
    assert!(matches!(
        state.outgoing_send(101).map(|send| &send.state),
        Some(OutgoingSendState::Failed {
            retryable: true,
            ..
        })
    ));

    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
        101, 2, "hello",
    )));
    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sent(
        101, 2, "hello", 55,
    )));
    assert!(state.outgoing_send(101).is_none());
}

#[test]
fn core_state_rejects_client_id_collision_while_request_is_in_outbox() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
        15, 2, "first",
    )));
    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
        15,
        2,
        "different",
    )));

    assert_eq!(state.outgoing_send(15).unwrap().text, "first");
    assert!(state
        .last_error
        .as_deref()
        .is_some_and(|message| message.contains("reused")));
}

#[test]
fn acknowledged_client_id_remains_bound_to_its_original_payload() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
        16, 2, "first",
    )));
    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sent(
        16, 2, "first", 99,
    )));
    assert!(state.outbox.is_empty());

    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
        16,
        2,
        "different",
    )));
    assert_eq!(state.send_ledger.get(&16).unwrap().text, "first");
    assert!(state
        .last_error
        .as_deref()
        .is_some_and(|message| message.contains("reused")));
}

#[derive(Clone)]
struct WriteAheadBackend {
    store: MemoryStateStore,
    observed: Arc<Mutex<Option<OutgoingSendState>>>,
}

impl TelegramBackend for WriteAheadBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        let store = self.store.clone();
        let observed = Arc::clone(&self.observed);
        Box::pin(async move {
            if let CoreCommand::SendTextReliable {
                client_id,
                chat_id,
                reply_to_message_id,
                text,
            } = command
            {
                let state = store
                    .snapshot()?
                    .and_then(|state| state.outbox.get(&client_id).cloned())
                    .map(|send| send.state);
                *observed.lock().unwrap() = state;
                Ok(vec![CoreEvent::MessageSendStateChanged(
                    OutgoingSend::sent(client_id, chat_id, text, 77),
                )])
            } else {
                Ok(Vec::new())
            }
        })
    }
}

#[tokio::test]
async fn send_intent_is_persisted_before_backend_rpc_starts() {
    let store = MemoryStateStore::default();
    let observed = Arc::new(Mutex::new(None));
    let core = spawn_core_with_store(
        WriteAheadBackend {
            store: store.clone(),
            observed: Arc::clone(&observed),
        },
        store,
    );
    let mut events = core.subscribe();
    core.send(CoreCommand::SendTextReliable {
        client_id: 17,
        chat_id: 2,

        reply_to_message_id: None,
        text: "write ahead".into(),
    })
    .await
    .unwrap();

    timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                events.recv().await.unwrap(),
                CoreEvent::MessageSendStateChanged(OutgoingSend {
                    state: OutgoingSendState::Sent { .. },
                    ..
                })
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();

    assert_eq!(*observed.lock().unwrap(), Some(OutgoingSendState::Sending));
    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn acknowledged_client_id_binding_survives_restart() {
    let store = MemoryStateStore::default();
    let first_observation = Arc::new(Mutex::new(None));
    let first_core = spawn_core_with_store(
        WriteAheadBackend {
            store: store.clone(),
            observed: Arc::clone(&first_observation),
        },
        store.clone(),
    );
    let mut first_events = first_core.subscribe();
    first_core
        .send(CoreCommand::SendTextReliable {
            client_id: 19,
            chat_id: 2,

            reply_to_message_id: None,
            text: "accepted".into(),
        })
        .await
        .unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                first_events.recv().await.unwrap(),
                CoreEvent::MessageSendStateChanged(OutgoingSend {
                    state: OutgoingSendState::Sent { .. },
                    ..
                })
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        *first_observation.lock().unwrap(),
        Some(OutgoingSendState::Sending)
    );
    first_core.shutdown().await.unwrap();

    let retry_observation = Arc::new(Mutex::new(None));
    let restarted = spawn_core_with_store(
        WriteAheadBackend {
            store: store.clone(),
            observed: Arc::clone(&retry_observation),
        },
        store.clone(),
    );
    let mut retry_events = restarted.subscribe();
    restarted
        .send(CoreCommand::SendTextReliable {
            client_id: 19,
            chat_id: 2,

            reply_to_message_id: None,
            text: "accepted".into(),
        })
        .await
        .unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                retry_events.recv().await.unwrap(),
                CoreEvent::MessageSendStateChanged(OutgoingSend {
                    state: OutgoingSendState::Sent { .. },
                    ..
                })
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();

    restarted
        .send(CoreCommand::SendTextReliable {
            client_id: 19,
            chat_id: 2,

            reply_to_message_id: None,
            text: "changed after restart".into(),
        })
        .await
        .unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if matches!(retry_events.recv().await.unwrap(), CoreEvent::Error(_)) {
                break;
            }
        }
    })
    .await
    .unwrap();

    assert_eq!(*retry_observation.lock().unwrap(), None);
    let persisted = store.snapshot().unwrap().unwrap();
    assert_eq!(persisted.send_ledger.get(&19).unwrap().text, "accepted");
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn restart_recovers_in_flight_send_as_awaiting_confirmation() {
    let store = MemoryStateStore::default();
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
        18,
        2,
        "uncertain",
    )));
    store.save(&state).await.unwrap();

    let core = spawn_core_with_store(MockTelegramBackend::default(), store.clone());
    let mut snapshot = core.subscribe_state();
    timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                snapshot.borrow().outgoing_send(18).map(|send| &send.state),
                Some(OutgoingSendState::AwaitingConfirmation { .. })
            ) {
                break;
            }
            snapshot.changed().await.unwrap();
        }
    })
    .await
    .unwrap();

    let persisted = store.snapshot().unwrap().unwrap();
    assert!(matches!(
        persisted.outgoing_send(18).map(|send| &send.state),
        Some(OutgoingSendState::AwaitingConfirmation { .. })
    ));
    core.shutdown().await.unwrap();
}

#[test]
fn old_persisted_state_without_outbox_still_deserializes() {
    let json = r#"{
        "running": true,
        "account": {"authorization":"Closed","chats":[]},
        "messages": {},
        "last_error": null
    }"#;

    let state: CoreState = serde_json::from_str(json).unwrap();
    assert!(state.outbox.is_empty());
}

#[test]
fn reliable_send_wire_command_is_additive_and_validated() {
    let legacy =
        decode_command(r#"{"type":"send_text","payload":{"chat_id":2,"text":"legacy"}}"#).unwrap();
    assert_eq!(legacy.kind(), "send_text");

    let reliable = decode_command(
        r#"{"type":"send_text_reliable","payload":{"client_id":42,"chat_id":2,"text":"hello"}}"#,
    )
    .unwrap();
    assert_eq!(reliable.kind(), "send_text_reliable");
    assert!(reliable.validate().is_ok());

    let invalid = CoreCommand::SendTextReliable {
        client_id: 0,
        chat_id: 2,

        reply_to_message_id: None,
        text: "hello".into(),
    };
    assert!(invalid.validate().is_err());
}

#[derive(Clone)]
struct AmbiguousRetryBackend {
    calls: Arc<Mutex<Vec<CoreCommand>>>,
}

impl TelegramBackend for AmbiguousRetryBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        let calls = Arc::clone(&self.calls);
        Box::pin(async move {
            match command {
                CoreCommand::SendTextReliable {
                    client_id,
                    chat_id,
                    reply_to_message_id,
                    text,
                } => {
                    calls.lock().unwrap().push(CoreCommand::SendTextReliable {
                        client_id,
                        chat_id,
                        reply_to_message_id,
                        text: text.clone(),
                    });
                    if calls.lock().unwrap().len() == 1 {
                        Err(CoreError::SendOutcomeUnknown(
                            "transport closed after request was sent".into(),
                        ))
                    } else {
                        Ok(vec![CoreEvent::MessageSendStateChanged(
                            OutgoingSend::sent(client_id, chat_id, text, 700)
                                .with_reply_target(reply_to_message_id),
                        )])
                    }
                }
                _ => Ok(Vec::new()),
            }
        })
    }
}

#[tokio::test]
async fn ambiguous_send_retry_reuses_the_same_client_id_and_payload() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let core = spawn_core_with_store(
        AmbiguousRetryBackend {
            calls: Arc::clone(&calls),
        },
        MemoryStateStore::default(),
    );
    let mut state = core.subscribe_state();
    let command = CoreCommand::SendTextReliable {
        client_id: 777,
        chat_id: 42,

        reply_to_message_id: None,
        text: "retry exactly this payload".into(),
    };

    core.send(command.clone()).await.unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                state.borrow().outgoing_send(777).map(|send| &send.state),
                Some(OutgoingSendState::AwaitingConfirmation { .. })
            ) {
                break;
            }
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();

    core.send(command.clone()).await.unwrap();
    let sent = timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                state.borrow().send_ledger.get(&777).map(|send| &send.state),
                Some(OutgoingSendState::Sent { message_id: 700 })
            ) {
                break;
            }
            state.changed().await.unwrap();
        }
    })
    .await;
    assert!(
        sent.is_ok(),
        "retry did not finish: state={:?}, calls={:?}",
        state.borrow().send_ledger.get(&777),
        calls.lock().unwrap()
    );

    assert_eq!(*calls.lock().unwrap(), vec![command.clone(), command]);
    core.shutdown().await.unwrap();
}

#[derive(Clone, Copy)]
enum SendResponseOrder {
    UpdateBeforeAck,
    AckBeforeUpdate,
    DuplicateAck,
    MissingAck,
}

struct OrderedSendBackend {
    response_order: SendResponseOrder,
    calls: Arc<Mutex<Vec<CoreCommand>>>,
    updates_tx: BackendUpdateSender,
    updates_rx: Option<BackendUpdateReceiver>,
}

impl OrderedSendBackend {
    fn new(
        response_order: SendResponseOrder,
    ) -> (Self, BackendUpdateSender, Arc<Mutex<Vec<CoreCommand>>>) {
        let (updates_tx, updates_rx) = backend_update_channel();
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                response_order,
                calls: Arc::clone(&calls),
                updates_tx: updates_tx.clone(),
                updates_rx: Some(updates_rx),
            },
            updates_tx,
            calls,
        )
    }
}

impl TelegramBackend for OrderedSendBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        let calls = Arc::clone(&self.calls);
        let response_order = self.response_order;
        let updates_tx = self.updates_tx.clone();
        Box::pin(async move {
            let CoreCommand::SendTextReliable {
                client_id,
                chat_id,
                reply_to_message_id,
                text,
            } = command
            else {
                return Ok(Vec::new());
            };
            calls.lock().unwrap().push(CoreCommand::SendTextReliable {
                client_id,
                chat_id,
                reply_to_message_id,
                text: text.clone(),
            });

            let message = test_message(900, chat_id, text.clone());
            let sent = CoreEvent::MessageSendStateChanged(
                OutgoingSend::sent(client_id, chat_id, text, message.id)
                    .with_reply_target(reply_to_message_id),
            );
            match response_order {
                SendResponseOrder::UpdateBeforeAck => {
                    updates_tx
                        .send(Ok(CoreEvent::MessageAdded(message)))
                        .await
                        .map_err(|_| CoreError::Backend("update receiver closed".into()))?;
                    Ok(vec![sent])
                }
                SendResponseOrder::AckBeforeUpdate => Ok(vec![sent]),
                SendResponseOrder::DuplicateAck => {
                    Ok(vec![CoreEvent::MessageAdded(message), sent.clone(), sent])
                }
                SendResponseOrder::MissingAck => Ok(Vec::new()),
            }
        })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        self.updates_rx.take()
    }
}

#[tokio::test]
async fn successful_backend_call_without_matching_ack_awaits_confirmation() {
    let (backend, _, calls) = OrderedSendBackend::new(SendResponseOrder::MissingAck);
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    let mut state = core.subscribe_state();
    let command = reliable_send_command();
    core.send(command.clone()).await.unwrap();

    wait_until(&mut state, |state| {
        matches!(
            state.send_ledger.get(&880).map(|send| &send.state),
            Some(OutgoingSendState::AwaitingConfirmation { .. })
        )
    })
    .await;

    assert_eq!(*calls.lock().unwrap(), vec![command]);
    assert!(state.borrow().outbox.contains_key(&880));
    core.shutdown().await.unwrap();
}

fn test_message(id: i64, chat_id: i64, text: String) -> MessageSummary {
    MessageSummary {
        id,
        chat_id,
        sender_id: 5,
        text,
        timestamp_unix: 1_700_000_000,
        outgoing: true,
        location: None,
        attachments: Vec::new(),
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

async fn wait_until(
    state: &mut tokio::sync::watch::Receiver<CoreState>,
    predicate: impl Fn(&CoreState) -> bool,
) {
    timeout(Duration::from_secs(2), async {
        loop {
            if predicate(&state.borrow()) {
                break;
            }
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

fn reliable_send_command() -> CoreCommand {
    CoreCommand::SendTextReliable {
        client_id: 880,
        chat_id: 42,

        reply_to_message_id: None,
        text: "ordered response".into(),
    }
}

#[test]
fn reliable_reply_payload_is_validated_and_preserved_in_legacy_compatible_state() {
    let command = CoreCommand::SendTextReliable {
        client_id: 901,
        chat_id: 42,
        reply_to_message_id: Some(77),
        text: "reply body".into(),
    };
    assert!(command.validate().is_ok());

    let invalid = CoreCommand::SendTextReliable {
        client_id: 902,
        chat_id: 42,
        reply_to_message_id: Some(0),
        text: "reply body".into(),
    };
    assert!(invalid.validate().is_err());

    let legacy: CoreCommand = serde_json::from_value(serde_json::json!({
        "type": "send_text_reliable",
        "payload": {"client_id": 903, "chat_id": 42, "text": "old wire"}
    }))
    .unwrap();
    assert!(matches!(
        legacy,
        CoreCommand::SendTextReliable {
            reply_to_message_id: None,
            ..
        }
    ));

    let send = OutgoingSend::sending(901, 42, "reply body").with_reply_target(Some(77));
    let encoded = serde_json::to_value(&send).unwrap();
    let restored: OutgoingSend = serde_json::from_value(encoded).unwrap();
    assert_eq!(restored.reply_to_message_id, Some(77));
    assert!(restored.same_request(42, "reply body", Some(77)));
    assert!(!restored.same_request(42, "reply body", Some(78)));
}

#[tokio::test]
async fn update_channel_message_sent_before_rpc_ack_keeps_one_message_and_clears_outbox() {
    let (backend, _, calls) = OrderedSendBackend::new(SendResponseOrder::UpdateBeforeAck);
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    let mut state = core.subscribe_state();
    let command = reliable_send_command();
    core.send(command.clone()).await.unwrap();

    wait_until(&mut state, |state| {
        state.messages_for_chat(42).len() == 1
            && state.outbox.is_empty()
            && matches!(
                state.send_ledger.get(&880).map(|send| &send.state),
                Some(OutgoingSendState::Sent { message_id: 900 })
            )
    })
    .await;
    assert_eq!(state.borrow().messages_for_chat(42).len(), 1);
    assert!(state.borrow().outbox.is_empty());
    assert_eq!(*calls.lock().unwrap(), vec![command]);
    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn rpc_ack_before_message_update_keeps_one_message_and_clears_outbox() {
    let (backend, updates_tx, calls) = OrderedSendBackend::new(SendResponseOrder::AckBeforeUpdate);
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    let state = core.subscribe_state();
    let mut events = core.subscribe();
    let command = reliable_send_command();
    core.send(command.clone()).await.unwrap();

    timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                events.recv().await.unwrap(),
                CoreEvent::MessageSendStateChanged(OutgoingSend {
                    client_id: 880,
                    state: OutgoingSendState::Sent { message_id: 900 },
                    ..
                })
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();

    updates_tx
        .send(Ok(CoreEvent::MessageAdded(test_message(
            900,
            42,
            "ordered response".into(),
        ))))
        .await
        .unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                events.recv().await.unwrap(),
                CoreEvent::MessageAdded(message) if message.id == 900
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();

    assert_eq!(state.borrow().messages_for_chat(42).len(), 1);
    assert!(state.borrow().outbox.is_empty());
    assert!(matches!(
        state.borrow().send_ledger.get(&880).map(|send| &send.state),
        Some(OutgoingSendState::Sent { message_id: 900 })
    ));
    assert_eq!(*calls.lock().unwrap(), vec![command]);
    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn duplicate_rpc_ack_and_replayed_command_do_not_duplicate_send() {
    let (backend, _, calls) = OrderedSendBackend::new(SendResponseOrder::DuplicateAck);
    let core = spawn_core_with_store(backend, MemoryStateStore::default());
    let state = core.subscribe_state();
    let mut events = core.subscribe();
    let command = reliable_send_command();
    core.send(command.clone()).await.unwrap();
    let mut ack_count = 0;
    let mut message_count = 0;
    timeout(Duration::from_secs(2), async {
        while ack_count < 2 || message_count < 1 {
            match events.recv().await.unwrap() {
                CoreEvent::MessageSendStateChanged(OutgoingSend {
                    client_id: 880,
                    state: OutgoingSendState::Sent { .. },
                    ..
                }) => ack_count += 1,
                CoreEvent::MessageAdded(message) if message.id == 900 => message_count += 1,
                _ => {}
            }
        }
    })
    .await
    .unwrap();

    core.send(command.clone()).await.unwrap();
    timeout(Duration::from_secs(2), async {
        while ack_count < 3 {
            if matches!(
                events.recv().await.unwrap(),
                CoreEvent::MessageSendStateChanged(OutgoingSend {
                    client_id: 880,
                    state: OutgoingSendState::Sent { .. },
                    ..
                })
            ) {
                ack_count += 1;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(state.borrow().messages_for_chat(42).len(), 1);
    assert_eq!(state.borrow().send_ledger.len(), 1);
    assert!(state.borrow().outbox.is_empty());
    assert_eq!(*calls.lock().unwrap(), vec![command]);
    core.shutdown().await.unwrap();
}
