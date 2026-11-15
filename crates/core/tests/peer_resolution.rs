use std::time::Duration;

use tg_mobile_core::{
    spawn_core, CoreCommand, CoreError, CoreEvent, MessageLinkMetadata, MockTelegramBackend,
    PeerResolution,
};
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsupported_mock_resolution_is_an_explicit_error() {
    let core = spawn_core(MockTelegramBackend::default());
    let mut events = core.subscribe();

    core.send(CoreCommand::ResolvePeer {
        input: "@example_user".into(),
    })
    .await
    .unwrap();

    let event = timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(event, CoreEvent::Error(message) if message.contains("peer resolution is unsupported"))
    );
}

#[test]
fn resolve_command_rejects_malformed_input_before_backend() {
    let error = CoreCommand::ResolvePeer {
        input: "https://t.me.evil/example_user".into(),
    }
    .validate()
    .unwrap_err();

    assert!(matches!(error, CoreError::InvalidInput(_)));
}

#[test]
fn new_optional_message_metadata_has_wire_defaults() {
    let decoded: MessageLinkMetadata = serde_json::from_str("{}").unwrap();
    assert_eq!(decoded, MessageLinkMetadata::default());
}

#[test]
fn unresolved_peer_outcomes_are_distinct_on_the_wire() {
    let outcomes = [
        PeerResolution::Unknown {
            reason: "access hash unavailable".into(),
        },
        PeerResolution::NotFound,
        PeerResolution::Unsupported {
            reason: "private link needs a known peer".into(),
        },
    ];
    let wire: Vec<_> = outcomes
        .iter()
        .map(|outcome| serde_json::to_value(outcome).unwrap())
        .collect();
    assert_eq!(wire[0]["type"], "unknown");
    assert_eq!(wire[1]["type"], "not_found");
    assert_eq!(wire[2]["type"], "unsupported");
    for outcome in outcomes {
        let encoded = serde_json::to_string(&outcome).unwrap();
        let decoded: PeerResolution = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, outcome);
    }
}
