use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use tg_mobile_core::{
    backend::{BackendFuture, BackendUpdateReceiver},
    encode_event, spawn_core_with_store, AuthorizationState, ChatPage, ConnectionState,
    CoreCommand, CoreEvent, CoreState, LinkEvent, MemoryStateStore, MessageSummary,
    MockTelegramBackend, StateStore, TelegramBackend,
};
use tokio::{sync::mpsc, time::timeout};

struct UpdateBackend {
    updates: Option<BackendUpdateReceiver>,
}

struct ReconnectBackend {
    updates: Option<BackendUpdateReceiver>,
    dialog_loads: Arc<AtomicUsize>,
    fail_initial: bool,
    omit_initial_result: bool,
    fail_recovery: bool,
    omit_recovery_result: bool,
}

struct FirstPageBackend {
    updates: Option<BackendUpdateReceiver>,
    fail_page: bool,
}

impl TelegramBackend for FirstPageBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async move {
            match command {
                CoreCommand::Start => Ok(vec![CoreEvent::AuthorizationChanged(
                    AuthorizationState::Ready,
                )]),
                CoreCommand::LoadChatsPage { cursor, .. } => {
                    if self.fail_page {
                        return Err(tg_mobile_core::CoreError::Backend(
                            "simulated first-page failure".into(),
                        ));
                    }
                    Ok(vec![CoreEvent::ChatsPageLoaded(ChatPage {
                        requested_cursor: cursor,
                        chats: Vec::new(),
                        next_cursor: None,
                    })])
                }
                _ => Ok(Vec::new()),
            }
        })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        self.updates.take()
    }
}

impl TelegramBackend for ReconnectBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        let events = match command {
            CoreCommand::Start => vec![CoreEvent::AuthorizationChanged(AuthorizationState::Ready)],
            CoreCommand::LoadChats { .. } => {
                let load = self.dialog_loads.fetch_add(1, Ordering::SeqCst) + 1;
                if load == 1 && self.fail_initial {
                    return Box::pin(async {
                        Err(tg_mobile_core::CoreError::Backend(
                            "simulated initial dialog load failure".into(),
                        ))
                    });
                }
                if load > 1 && self.fail_recovery {
                    return Box::pin(async {
                        Err(tg_mobile_core::CoreError::Backend(
                            "simulated reconnect dialog load failure".into(),
                        ))
                    });
                }
                if (load == 1 && self.omit_initial_result)
                    || (load > 1 && self.omit_recovery_result)
                {
                    Vec::new()
                } else {
                    vec![CoreEvent::ChatsLoaded(Vec::new())]
                }
            }
            _ => Vec::new(),
        };
        Box::pin(async move { Ok(events) })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        self.updates.take()
    }
}

impl TelegramBackend for UpdateBackend {
    fn handle<'a>(&'a mut self, _command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        self.updates.take()
    }
}

#[test]
fn connection_event_has_stable_wire_shape_and_reduces_into_state() {
    let event = CoreEvent::ConnectionStateChanged(ConnectionState::Connecting);
    assert_eq!(event.kind(), "connection_state_changed");
    assert_eq!(
        encode_event(&event).unwrap(),
        r#"{"type":"connection_state_changed","payload":"connecting"}"#
    );

    let mut state = CoreState::default();
    assert_eq!(state.connection_state, ConnectionState::Disconnected);

    state.reduce(&event);
    assert_eq!(state.connection_state, ConnectionState::Disconnected);

    state.reduce(&CoreEvent::ConnectionSessionStarted);
    let token = state.connection_machine.token();
    state.reduce(&CoreEvent::ConnectionLifecycle {
        token,
        event: LinkEvent::TransportConnected,
    });
    state.reduce(&CoreEvent::ConnectionLifecycle {
        token,
        event: LinkEvent::SyncStarted,
    });
    state.reduce(&CoreEvent::ConnectionLifecycle {
        token,
        event: LinkEvent::SyncCompleted,
    });
    assert_eq!(state.connection_state, ConnectionState::Ready);
}

#[test]
fn legacy_persisted_state_defaults_connection_to_disconnected() {
    let legacy = r#"{
        "running": true,
        "account": {"authorization":"Closed","chats":[]},
        "messages": {},
        "last_error": null
    }"#;

    let state: CoreState = serde_json::from_str(legacy).unwrap();
    assert_eq!(state.connection_state, ConnectionState::Disconnected);
}

#[tokio::test]
async fn restored_snapshot_never_reports_stale_ready_connection() {
    let store = MemoryStateStore::default();
    let mut persisted = CoreState::default();
    persisted.account.authorization = AuthorizationState::Ready;
    persisted.connection_state = ConnectionState::Ready;
    persisted.reduce(&CoreEvent::ConnectionSessionStarted);
    let token = persisted.connection_machine.token();
    for event in [
        LinkEvent::TransportConnected,
        LinkEvent::SyncStarted,
        LinkEvent::SyncCompleted,
    ] {
        persisted.reduce(&CoreEvent::ConnectionLifecycle { token, event });
    }
    store.save(&persisted).await.unwrap();

    let core = spawn_core_with_store(MockTelegramBackend::default(), store);

    timeout(Duration::from_secs(1), async {
        loop {
            let state = core.state();
            if state.account.authorization == AuthorizationState::Ready {
                assert_eq!(state.connection_state, ConnectionState::Disconnected);
                assert_eq!(
                    state.connection_machine.phase(),
                    tg_mobile_core::LinkPhase::Disconnected
                );
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    core.shutdown().await.unwrap();
}

#[test]
fn connection_states_round_trip_through_json() {
    for state in [
        ConnectionState::Connecting,
        ConnectionState::Ready,
        ConnectionState::Disconnected,
    ] {
        let encoded = serde_json::to_string(&state).unwrap();
        let decoded: ConnectionState = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, state);
    }
}

#[test]
fn stale_connection_and_backend_updates_cannot_change_a_new_session() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ConnectionSessionStarted);
    let old_token = state.connection_machine.token();
    for event in [
        LinkEvent::TransportConnected,
        LinkEvent::SyncStarted,
        LinkEvent::SyncCompleted,
    ] {
        state.reduce(&CoreEvent::ConnectionLifecycle {
            token: old_token,
            event,
        });
    }
    state.reduce(&CoreEvent::ConnectionLifecycle {
        token: old_token,
        event: LinkEvent::TransportLost,
    });
    let reconnect_token = state.connection_machine.token();
    assert_eq!(
        state.connection_machine.phase(),
        tg_mobile_core::LinkPhase::Reconnecting
    );
    assert_ne!(reconnect_token, old_token);

    state.reduce(&CoreEvent::SessionUpdate {
        token: old_token,
        event: Box::new(CoreEvent::Error("stale worker error".into())),
    });
    assert_eq!(
        state.connection_machine.phase(),
        tg_mobile_core::LinkPhase::Reconnecting
    );
    assert_eq!(state.last_error, None);

    state.reduce(&CoreEvent::ConnectionLifecycle {
        token: reconnect_token,
        event: LinkEvent::Logout,
    });
    state.reduce(&CoreEvent::ConnectionSessionStarted);
    let new_token = state.connection_machine.token();
    assert_ne!(new_token.generation(), old_token.generation());
    state.reduce(&CoreEvent::SessionUpdate {
        token: old_token,
        event: Box::new(CoreEvent::ConnectionLifecycle {
            token: old_token,
            event: LinkEvent::SyncCompleted,
        }),
    });
    assert_eq!(
        state.connection_machine.phase(),
        tg_mobile_core::LinkPhase::Connecting
    );
}

#[test]
fn successful_dialog_load_is_the_initial_sync_boundary() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ConnectionSessionStarted);
    let token = state.connection_machine.token();
    state.reduce(&CoreEvent::ConnectionLifecycle {
        token,
        event: LinkEvent::TransportConnected,
    });
    state.reduce(&CoreEvent::ConnectionLifecycle {
        token,
        event: LinkEvent::SyncStarted,
    });

    assert_eq!(
        state.connection_machine.phase(),
        tg_mobile_core::LinkPhase::Synchronizing
    );
    assert_eq!(state.connection_state, ConnectionState::Connecting);

    state.reduce(&CoreEvent::ChatsLoaded(Vec::new()));

    assert_eq!(
        state.connection_machine.phase(),
        tg_mobile_core::LinkPhase::Ready
    );
    assert_eq!(state.connection_state, ConnectionState::Ready);
}

#[test]
fn only_a_valid_first_chat_page_completes_reducer_initial_sync() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ConnectionSessionStarted);
    let token = state.connection_machine.token();
    state.reduce(&CoreEvent::ConnectionLifecycle {
        token,
        event: LinkEvent::TransportConnected,
    });
    state.reduce(&CoreEvent::ConnectionLifecycle {
        token,
        event: LinkEvent::SyncStarted,
    });
    state.reduce(&CoreEvent::ChatsPageLoading { cursor: None });
    state.reduce(&CoreEvent::ChatsPageLoaded(ChatPage {
        requested_cursor: Some("stale".into()),
        chats: Vec::new(),
        next_cursor: None,
    }));
    assert_eq!(
        state.connection_machine.phase(),
        tg_mobile_core::LinkPhase::Synchronizing
    );

    state.reduce(&CoreEvent::ChatsPageLoaded(ChatPage {
        requested_cursor: None,
        chats: Vec::new(),
        next_cursor: Some("peer-1".into()),
    }));
    assert_eq!(
        state.connection_machine.phase(),
        tg_mobile_core::LinkPhase::Ready
    );
    assert_eq!(state.connection_state, ConnectionState::Ready);
}

#[tokio::test]
async fn runtime_completes_sync_on_first_page_and_fails_only_when_page_fails() {
    for fail_page in [false, true] {
        let (updates_tx, updates_rx) = mpsc::channel(8);
        let core = spawn_core_with_store(
            FirstPageBackend {
                updates: Some(updates_rx),
                fail_page,
            },
            MemoryStateStore::default(),
        );
        let mut events = core.subscribe();
        core.send(CoreCommand::Start).await.unwrap();
        let token = timeout(Duration::from_secs(1), async {
            loop {
                let state = core.state();
                if state.account.authorization == AuthorizationState::Ready {
                    break state.connection_machine.token();
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        send_lifecycle(&updates_tx, token, LinkEvent::TransportConnected).await;
        send_lifecycle(&updates_tx, token, LinkEvent::SyncStarted).await;
        timeout(Duration::from_secs(1), async {
            loop {
                if core.state().connection_machine.phase()
                    == tg_mobile_core::LinkPhase::Synchronizing
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        core.send(CoreCommand::LoadChatsPage {
            cursor: None,
            limit: 100,
        })
        .await
        .unwrap();
        let expected_phase = if fail_page {
            tg_mobile_core::LinkPhase::Failed
        } else {
            tg_mobile_core::LinkPhase::Ready
        };
        timeout(Duration::from_secs(1), async {
            loop {
                if core.state().connection_machine.phase() == expected_phase {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        if !fail_page {
            assert!(core.state().chat_pagination.loaded);
            let mut connect_failed = false;
            while let Ok(event) = events.try_recv() {
                connect_failed |= matches!(
                    event,
                    CoreEvent::ConnectionLifecycle {
                        event: LinkEvent::ConnectFailed { .. },
                        ..
                    }
                );
            }
            assert!(!connect_failed);
        }
        core.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn actor_starts_and_stops_connection_machine_with_session_commands() {
    let core = spawn_core_with_store(MockTelegramBackend::default(), MemoryStateStore::default());
    core.send(tg_mobile_core::CoreCommand::Start).await.unwrap();

    timeout(Duration::from_secs(1), async {
        loop {
            if core.state().account.authorization == AuthorizationState::WaitingForPhoneNumber {
                assert_eq!(core.state().connection_state, ConnectionState::Connecting);
                assert_eq!(
                    core.state().connection_machine.phase(),
                    tg_mobile_core::LinkPhase::Connecting
                );
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    core.shutdown().await.unwrap();
    timeout(Duration::from_secs(1), async {
        while core.state().running {
            tokio::task::yield_now().await;
        }
        assert_eq!(core.state().connection_state, ConnectionState::Disconnected);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn runtime_drops_updates_from_the_previous_session_generation() {
    let (updates_tx, updates_rx) = mpsc::channel(8);
    let core = spawn_core_with_store(
        UpdateBackend {
            updates: Some(updates_rx),
        },
        MemoryStateStore::default(),
    );

    core.send(CoreCommand::Start).await.unwrap();
    let first = timeout(Duration::from_secs(1), async {
        loop {
            let state = core.state();
            if state.connection_machine.phase() == tg_mobile_core::LinkPhase::Connecting {
                break state.connection_machine.token();
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    updates_tx
        .send(Ok(CoreEvent::SessionUpdate {
            token: first,
            event: Box::new(CoreEvent::ConnectionLifecycle {
                token: first,
                event: LinkEvent::Logout,
            }),
        }))
        .await
        .unwrap();
    timeout(Duration::from_secs(1), async {
        loop {
            if core.state().connection_machine.phase() == tg_mobile_core::LinkPhase::Disconnected {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    core.send(CoreCommand::Start).await.unwrap();
    let second = timeout(Duration::from_secs(1), async {
        loop {
            let state = core.state();
            let token = state.connection_machine.token();
            if state.connection_machine.phase() == tg_mobile_core::LinkPhase::Connecting
                && token.generation() > first.generation()
            {
                break token;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let message = MessageSummary {
        id: 1,
        chat_id: 2,
        sender_id: 3,
        text: "old session".into(),
        timestamp_unix: 4,
        outgoing: false,
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
    };
    updates_tx
        .send(Ok(CoreEvent::SessionUpdate {
            token: first,
            event: Box::new(CoreEvent::MessageAdded(message.clone())),
        }))
        .await
        .unwrap();
    tokio::task::yield_now().await;
    assert!(core.state().messages_for_chat(2).is_empty());

    updates_tx
        .send(Ok(CoreEvent::SessionUpdate {
            token: second,
            event: Box::new(CoreEvent::MessageAdded(MessageSummary {
                text: "current session".into(),
                ..message
            })),
        }))
        .await
        .unwrap();
    timeout(Duration::from_secs(1), async {
        loop {
            let state = core.state();
            let messages = state.messages_for_chat(2);
            if messages
                .first()
                .is_some_and(|message| message.text == "current session")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn reconnect_triggers_actor_owned_dialog_sync_without_user_action() {
    let (updates_tx, updates_rx) = mpsc::channel(8);
    let dialog_loads = Arc::new(AtomicUsize::new(0));
    let core = spawn_core_with_store(
        ReconnectBackend {
            updates: Some(updates_rx),
            dialog_loads: Arc::clone(&dialog_loads),
            fail_initial: false,
            omit_initial_result: false,
            fail_recovery: false,
            omit_recovery_result: false,
        },
        MemoryStateStore::default(),
    );

    core.send(CoreCommand::Start).await.unwrap();
    let first = timeout(Duration::from_secs(1), async {
        loop {
            let state = core.state();
            if state.account.authorization == AuthorizationState::Ready {
                break state.connection_machine.token();
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    send_lifecycle(&updates_tx, first, LinkEvent::TransportConnected).await;
    send_lifecycle(&updates_tx, first, LinkEvent::SyncStarted).await;
    timeout(Duration::from_secs(1), async {
        loop {
            if core.state().connection_machine.phase() == tg_mobile_core::LinkPhase::Synchronizing {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(dialog_loads.load(Ordering::SeqCst), 0);
    core.send(CoreCommand::LoadChats { limit: 100 })
        .await
        .unwrap();

    timeout(Duration::from_secs(1), async {
        loop {
            if core.state().connection_machine.phase() == tg_mobile_core::LinkPhase::Ready {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(dialog_loads.load(Ordering::SeqCst), 1);

    send_lifecycle(&updates_tx, first, LinkEvent::TransportLost).await;
    updates_tx
        .send(Ok(CoreEvent::SessionUpdate {
            token: first,
            event: Box::new(CoreEvent::Error("queued result from lost attempt".into())),
        }))
        .await
        .unwrap();
    let reconnect = timeout(Duration::from_secs(1), async {
        loop {
            let state = core.state();
            if state.connection_machine.phase() == tg_mobile_core::LinkPhase::Reconnecting {
                break state.connection_machine.token();
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_ne!(reconnect, first);
    assert_eq!(core.state().last_error, None);

    send_lifecycle(&updates_tx, reconnect, LinkEvent::Retry).await;
    send_lifecycle(&updates_tx, reconnect, LinkEvent::TransportConnected).await;
    send_lifecycle(&updates_tx, reconnect, LinkEvent::SyncStarted).await;

    timeout(Duration::from_secs(1), async {
        loop {
            if core.state().connection_machine.phase() == tg_mobile_core::LinkPhase::Ready {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(dialog_loads.load(Ordering::SeqCst), 2);
    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn recovery_sync_error_or_missing_result_transitions_to_failed() {
    for (fail_recovery, omit_recovery_result) in [(true, false), (false, true)] {
        let (updates_tx, updates_rx) = mpsc::channel(8);
        let dialog_loads = Arc::new(AtomicUsize::new(0));
        let core = spawn_core_with_store(
            ReconnectBackend {
                updates: Some(updates_rx),
                dialog_loads: Arc::clone(&dialog_loads),
                fail_initial: false,
                omit_initial_result: false,
                fail_recovery,
                omit_recovery_result,
            },
            MemoryStateStore::default(),
        );
        core.send(CoreCommand::Start).await.unwrap();
        let first = timeout(Duration::from_secs(1), async {
            loop {
                let state = core.state();
                if state.account.authorization == AuthorizationState::Ready {
                    break state.connection_machine.token();
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        send_lifecycle(&updates_tx, first, LinkEvent::TransportConnected).await;
        send_lifecycle(&updates_tx, first, LinkEvent::SyncStarted).await;
        timeout(Duration::from_secs(1), async {
            loop {
                if core.state().connection_machine.phase()
                    == tg_mobile_core::LinkPhase::Synchronizing
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        core.send(CoreCommand::LoadChats { limit: 100 })
            .await
            .unwrap();
        timeout(Duration::from_secs(1), async {
            loop {
                if core.state().connection_machine.phase() == tg_mobile_core::LinkPhase::Ready {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        send_lifecycle(&updates_tx, first, LinkEvent::TransportLost).await;
        let reconnect = timeout(Duration::from_secs(1), async {
            loop {
                let state = core.state();
                if state.connection_machine.phase() == tg_mobile_core::LinkPhase::Reconnecting {
                    break state.connection_machine.token();
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        send_lifecycle(&updates_tx, reconnect, LinkEvent::Retry).await;
        send_lifecycle(&updates_tx, reconnect, LinkEvent::TransportConnected).await;
        send_lifecycle(&updates_tx, reconnect, LinkEvent::SyncStarted).await;

        timeout(Duration::from_secs(1), async {
            loop {
                let state = core.state();
                if state.connection_machine.phase() == tg_mobile_core::LinkPhase::Failed {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(dialog_loads.load(Ordering::SeqCst), 2);
        core.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn initial_sync_error_or_missing_result_transitions_to_failed() {
    for (fail_initial, omit_initial_result) in [(true, false), (false, true)] {
        let (updates_tx, updates_rx) = mpsc::channel(8);
        let dialog_loads = Arc::new(AtomicUsize::new(0));
        let core = spawn_core_with_store(
            ReconnectBackend {
                updates: Some(updates_rx),
                dialog_loads: Arc::clone(&dialog_loads),
                fail_initial,
                omit_initial_result,
                fail_recovery: false,
                omit_recovery_result: false,
            },
            MemoryStateStore::default(),
        );
        core.send(CoreCommand::Start).await.unwrap();
        let token = timeout(Duration::from_secs(1), async {
            loop {
                let state = core.state();
                if state.account.authorization == AuthorizationState::Ready {
                    break state.connection_machine.token();
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        send_lifecycle(&updates_tx, token, LinkEvent::TransportConnected).await;
        send_lifecycle(&updates_tx, token, LinkEvent::SyncStarted).await;
        timeout(Duration::from_secs(1), async {
            loop {
                if core.state().connection_machine.phase()
                    == tg_mobile_core::LinkPhase::Synchronizing
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        core.send(CoreCommand::LoadChats { limit: 100 })
            .await
            .unwrap();
        timeout(Duration::from_secs(1), async {
            loop {
                if core.state().connection_machine.phase() == tg_mobile_core::LinkPhase::Failed {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(dialog_loads.load(Ordering::SeqCst), 1);
        core.shutdown().await.unwrap();
    }
}

async fn send_lifecycle(
    updates: &mpsc::Sender<tg_mobile_core::Result<CoreEvent>>,
    token: tg_mobile_core::ConnectionToken,
    event: LinkEvent,
) {
    updates
        .send(Ok(CoreEvent::SessionUpdate {
            token,
            event: Box::new(CoreEvent::ConnectionLifecycle { token, event }),
        }))
        .await
        .unwrap();
}
