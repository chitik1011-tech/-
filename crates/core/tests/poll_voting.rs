use std::time::Duration;

use tg_mobile_core::{
    backend::BackendFuture, spawn_core, CoreCommand, CoreError, CoreEvent, PollOption, PollSummary,
    TelegramBackend,
};
use tokio::time::timeout;

struct VoteFailureBackend;

impl TelegramBackend for VoteFailureBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async move {
            match command {
                CoreCommand::Start => Ok(vec![CoreEvent::PollUpdated(poll())]),
                CoreCommand::VotePoll { .. } => {
                    Err(CoreError::Backend("POLL_ANSWER_INVALID".into()))
                }
                CoreCommand::Shutdown => Ok(Vec::new()),
                _ => Ok(Vec::new()),
            }
        })
    }
}

fn poll() -> PollSummary {
    PollSummary {
        id: 99,
        chat_id: 42,
        message_id: 7,
        question: "Pick one".into(),
        options: vec![
            PollOption {
                text: "A".into(),
                option_id: b"a".to_vec(),
                voter_count: Some(2),
                chosen: false,
                correct: None,
            },
            PollOption {
                text: "B".into(),
                option_id: b"b".to_vec(),
                voter_count: Some(1),
                chosen: true,
                correct: None,
            },
        ],
        total_voters: Some(3),
        public_voters: false,
        multiple_choice: false,
        quiz: false,
        closed: false,
        explanation: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rpc_failure_is_correlated_and_keeps_server_confirmed_poll() {
    let core = spawn_core(VoteFailureBackend);
    let mut events = core.subscribe();

    core.send(CoreCommand::Start).await.unwrap();

    timeout(Duration::from_secs(1), async {
        loop {
            if matches!(events.recv().await.unwrap(), CoreEvent::PollUpdated(_)) {
                break;
            }
        }
    })
    .await
    .unwrap();

    let before = core.state().poll(99).cloned().unwrap();

    core.send(CoreCommand::VotePoll {
        poll_id: 99,
        chat_id: 42,
        message_id: 7,
        option_ids: vec![b"a".to_vec()],
    })
    .await
    .unwrap();

    let failure = timeout(Duration::from_secs(1), async {
        loop {
            if let CoreEvent::PollVoteFailed { poll_id, error } = events.recv().await.unwrap() {
                break (poll_id, error);
            }
        }
    })
    .await
    .unwrap();

    assert_eq!(failure.0, 99);
    assert!(failure.1.contains("POLL_ANSWER_INVALID"));
    assert_eq!(core.state().poll(99), Some(&before));
}
