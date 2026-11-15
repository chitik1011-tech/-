use tg_mobile_core::{CoreEvent, CoreState, Draft};

use std::time::Duration;

use tg_mobile_core::{
    spawn_core_with_store, AuthorizationState, CoreCommand, MemoryStateStore, MockTelegramBackend,
    StateStore,
};
use tokio::time::timeout;

fn draft(text: &str, updated_at_unix: i64) -> Draft {
    Draft {
        chat_id: 7,
        text: text.into(),
        reply_to_message_id: Some(3),
        edit_message_id: None,
        updated_at_unix,
    }
}

#[test]
fn draft_updates_are_idempotent_and_clear_is_version_guarded() {
    let mut state = CoreState::default();
    let first = draft("hello", 10);
    state.reduce(&CoreEvent::DraftUpdated(first.clone()));
    let snapshot = state.clone();
    state.reduce(&CoreEvent::DraftUpdated(first.clone()));
    assert_eq!(state, snapshot);

    let same_timestamp_newer = draft("new text", 10);
    state.reduce(&CoreEvent::DraftUpdated(same_timestamp_newer.clone()));
    state.reduce(&CoreEvent::DraftCleared {
        chat_id: 7,
        expected_updated_at_unix: Some(10),
        expected_draft: Some(first),
    });
    assert_eq!(state.drafts.get(&7), Some(&same_timestamp_newer));

    state.reduce(&CoreEvent::DraftCleared {
        chat_id: 7,
        expected_updated_at_unix: Some(10),
        expected_draft: Some(same_timestamp_newer.clone()),
    });
    assert!(!state.drafts.contains_key(&7));
    let cleared = state.clone();
    state.reduce(&CoreEvent::DraftCleared {
        chat_id: 7,
        expected_updated_at_unix: Some(10),
        expected_draft: Some(same_timestamp_newer),
    });
    assert_eq!(state, cleared);
}

#[test]
fn drafts_round_trip_in_persisted_state() {
    let mut state = CoreState::default();
    let value = draft("resume after restart", 42);
    state.drafts.insert(value.chat_id, value.clone());
    let encoded = serde_json::to_string(&state).unwrap();
    let restored: CoreState = serde_json::from_str(&encoded).unwrap();
    assert_eq!(restored.drafts.get(&7), Some(&value));
}

#[test]
fn persisted_draft_validation_rejects_key_mismatch_and_invalid_targets() {
    let mut state = CoreState::default();
    state.drafts.insert(8, draft("mismatched", 42));
    assert!(state.validate().is_err());

    state.drafts.clear();
    let mut invalid = draft("bad target", 42);
    invalid.reply_to_message_id = Some(0);
    state.drafts.insert(invalid.chat_id, invalid);
    assert!(state.validate().is_err());
}

#[test]
fn reducer_rejects_invalid_draft_events() {
    let mut state = CoreState::default();
    let mut invalid = draft("invalid reply", 42);
    invalid.reply_to_message_id = Some(0);

    state.reduce(&CoreEvent::DraftUpdated(invalid));

    assert!(state.drafts.is_empty());
    assert!(state.last_error.is_some());
    assert!(state.validate().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_persists_draft_updates_and_timestamp_guarded_clear() {
    let store = MemoryStateStore::default();
    let value = draft("saved through runtime", 42);
    let mut initial = CoreState::default();
    initial.account.authorization = AuthorizationState::Ready;
    store.save(&initial).await.unwrap();

    let first = spawn_core_with_store(MockTelegramBackend::default(), store.clone());
    wait_for_loaded(&first).await;
    first
        .send(CoreCommand::UpdateDraft(value.clone()))
        .await
        .unwrap();
    wait_for_draft(&first, true).await;
    first.shutdown().await.unwrap();
    wait_until_closed(&first).await;

    let second = spawn_core_with_store(MockTelegramBackend::default(), store.clone());
    wait_for_loaded(&second).await;
    wait_for_draft(&second, true).await;
    second
        .send(CoreCommand::ClearDraft {
            chat_id: value.chat_id,
            expected_updated_at_unix: Some(42),
            expected_draft: Some(draft("changed at same timestamp", 42)),
        })
        .await
        .unwrap();
    wait_for_draft(&second, true).await;
    second
        .send(CoreCommand::ClearDraft {
            chat_id: value.chat_id,
            expected_updated_at_unix: Some(value.updated_at_unix),
            expected_draft: Some(value.clone()),
        })
        .await
        .unwrap();
    wait_for_draft(&second, false).await;
    second.shutdown().await.unwrap();
    wait_until_closed(&second).await;

    let third = spawn_core_with_store(MockTelegramBackend::default(), store);
    wait_for_loaded(&third).await;
    wait_for_draft(&third, false).await;
    third.shutdown().await.unwrap();
    wait_until_closed(&third).await;
}

async fn wait_for_draft(core: &tg_mobile_core::CoreHandle, present: bool) {
    timeout(Duration::from_secs(2), async {
        loop {
            if core.state().drafts.contains_key(&7) == present {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("draft state should reach the expected value");
}

async fn wait_for_loaded(core: &tg_mobile_core::CoreHandle) {
    timeout(Duration::from_secs(2), async {
        while core.state().account.authorization != AuthorizationState::Ready {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("runtime should load stored state");
}

async fn wait_until_closed(core: &tg_mobile_core::CoreHandle) {
    timeout(Duration::from_secs(2), async {
        while !core.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("runtime should close after shutdown");
}
