use tg_mobile_core::{
    command::CoreCommand,
    event::CoreEvent,
    forwarding::{
        normalize_forward_events, ForwardMessagesRequest, ForwardMessagesResult, ForwardOutcome,
    },
    model::MessageRef,
    CoreState, MockTelegramBackend,
};
use tg_mobile_core::{spawn_core, BackendFuture, TelegramBackend};

fn source(chat_id: i64, message_id: i64) -> MessageRef {
    MessageRef {
        chat_id,
        message_id,
    }
}

fn request() -> ForwardMessagesRequest {
    ForwardMessagesRequest {
        sources: vec![source(10, 1), source(20, 2)],
        destination_chat_id: 30,
        request_id: "request-1".into(),
    }
}

#[test]
fn command_validates_source_count_refs_destination_and_correlation_id() {
    let valid = CoreCommand::ForwardMessages(request());
    assert!(valid.validate().is_ok());

    let mut invalid = request();
    invalid.sources.clear();
    assert!(CoreCommand::ForwardMessages(invalid).validate().is_err());

    let mut invalid = request();
    invalid.sources.push(source(10, 1));
    assert!(CoreCommand::ForwardMessages(invalid).validate().is_err());

    let mut invalid = request();
    invalid.destination_chat_id = 0;
    assert!(CoreCommand::ForwardMessages(invalid).validate().is_err());

    let mut invalid = request();
    invalid.request_id = "  ".into();
    assert!(CoreCommand::ForwardMessages(invalid).validate().is_err());

    let mut invalid = request();
    invalid.sources[0].message_id = 0;
    assert!(CoreCommand::ForwardMessages(invalid).validate().is_err());
}

#[test]
fn command_round_trips_through_wire_json() {
    let command = CoreCommand::ForwardMessages(request());
    let encoded = serde_json::to_string(&command).unwrap();
    let decoded: CoreCommand = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, command);
}

#[test]
fn runtime_normalization_orders_deduplicates_and_completes_partial_failures() {
    let req = request();
    let duplicate = CoreEvent::MessagesForwarded(ForwardMessagesResult {
        request_id: req.request_id.clone(),
        destination_chat_id: req.destination_chat_id,
        outcomes: vec![
            ForwardOutcome::Forwarded {
                source: source(20, 2),
                destination_message_id: 202,
            },
            ForwardOutcome::Forwarded {
                source: source(20, 2),
                destination_message_id: 999,
            },
            ForwardOutcome::Failed {
                source: source(10, 1),
                error: "restricted".into(),
            },
            ForwardOutcome::Forwarded {
                source: source(999, 9),
                destination_message_id: 909,
            },
        ],
    });
    let normalized = normalize_forward_events(&req, vec![duplicate]);
    let CoreEvent::MessagesForwarded(result) = &normalized[0] else {
        panic!("expected forwarding result")
    };
    assert_eq!(result.request_id, "request-1");
    assert_eq!(
        result.outcomes[0],
        ForwardOutcome::Failed {
            source: source(10, 1),
            error: "restricted".into()
        }
    );
    assert_eq!(
        result.outcomes[1],
        ForwardOutcome::Forwarded {
            source: source(20, 2),
            destination_message_id: 202
        }
    );
}

#[test]
fn invalid_backend_success_becomes_failure() {
    let req = request();
    let event = CoreEvent::MessagesForwarded(ForwardMessagesResult {
        request_id: req.request_id.clone(),
        destination_chat_id: req.destination_chat_id,
        outcomes: vec![ForwardOutcome::Forwarded {
            source: source(10, 1),
            destination_message_id: 0,
        }],
    });
    let normalized = normalize_forward_events(&req, vec![event]);
    let CoreEvent::MessagesForwarded(result) = &normalized[0] else {
        panic!("expected forwarding result")
    };
    assert!(
        matches!(&result.outcomes[0], ForwardOutcome::Failed { source: r, .. } if *r == source(10, 1))
    );
    assert!(
        matches!(&result.outcomes[1], ForwardOutcome::Failed { source: r, .. } if *r == source(20, 2))
    );
}

#[test]
fn reducer_deduplicates_correlated_results_and_older_snapshots_load() {
    let req = request();
    let result = ForwardMessagesResult {
        request_id: req.request_id.clone(),
        destination_chat_id: req.destination_chat_id,
        outcomes: vec![ForwardOutcome::Failed {
            source: source(10, 1),
            error: "restricted".into(),
        }],
    };
    let event = CoreEvent::MessagesForwarded(result.clone());
    let mut state = CoreState::default();
    state.reduce(&event);
    state.reduce(&CoreEvent::MessagesForwarded(ForwardMessagesResult {
        request_id: req.request_id.clone(),
        destination_chat_id: 30,
        outcomes: vec![ForwardOutcome::Forwarded {
            source: source(10, 1),
            destination_message_id: 111,
        }],
    }));
    assert_eq!(state.forwarding.get("request-1"), Some(&result));

    let mut old = serde_json::to_value(CoreState::default()).unwrap();
    old.as_object_mut().unwrap().remove("forwarding");
    let restored: CoreState = serde_json::from_value(old).unwrap();
    assert!(restored.forwarding.is_empty());
}

#[tokio::test]
async fn unsupported_mock_backend_returns_an_error_instead_of_forward_success() {
    let mut backend = MockTelegramBackend::default();
    let result = backend
        .handle(CoreCommand::ForwardMessages(request()))
        .await;
    assert!(result.is_err());
    assert!(backend.forward_messages(request()).await.is_err());

    let runtime = spawn_core(MockTelegramBackend::default());
    let mut events = runtime.subscribe();
    runtime
        .send(CoreCommand::ForwardMessages(request()))
        .await
        .unwrap();
    let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event, CoreEvent::Error(_)));
}

struct TypedForwardingBackend;

impl TelegramBackend for TypedForwardingBackend {
    fn handle<'a>(&'a mut self, _command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async {
            Err(tg_mobile_core::CoreError::Backend(
                "generic handler must not receive forward command".into(),
            ))
        })
    }

    fn forward_messages<'a>(
        &'a mut self,
        request: ForwardMessagesRequest,
    ) -> tg_mobile_core::backend::ForwardMessagesFuture<'a> {
        Box::pin(async move {
            Ok(ForwardMessagesResult {
                request_id: request.request_id,
                destination_chat_id: request.destination_chat_id,
                outcomes: vec![
                    ForwardOutcome::Forwarded {
                        source: request.sources[1].clone(),
                        destination_message_id: 302,
                    },
                    ForwardOutcome::Forwarded {
                        source: request.sources[1].clone(),
                        destination_message_id: 999,
                    },
                    ForwardOutcome::Failed {
                        source: request.sources[0].clone(),
                        error: "restricted".into(),
                    },
                ],
            })
        })
    }
}

#[tokio::test]
async fn runtime_uses_typed_forward_contract_and_emits_ordered_partial_results() {
    let runtime = spawn_core(TypedForwardingBackend);
    let mut events = runtime.subscribe();
    runtime
        .send(CoreCommand::ForwardMessages(request()))
        .await
        .unwrap();

    let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();
    let CoreEvent::MessagesForwarded(result) = event else {
        panic!("typed backend should return the forwarding event");
    };
    assert_eq!(result.request_id, "request-1");
    assert_eq!(
        result.outcomes,
        vec![
            ForwardOutcome::Failed {
                source: source(10, 1),
                error: "restricted".into()
            },
            ForwardOutcome::Forwarded {
                source: source(20, 2),
                destination_message_id: 302
            },
        ]
    );
}
