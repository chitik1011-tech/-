use tg_mobile_core::{
    CoreCommand, CoreEvent, CoreState, PollOption, PollSummary, MAX_CACHED_POLLS, MAX_POLL_OPTIONS,
};

const CHAT_ID: i64 = 42;

fn option(text: &str, voters: Option<u32>, chosen: bool) -> PollOption {
    PollOption {
        text: text.to_owned(),
        option_id: text.as_bytes().to_vec(),
        voter_count: voters,
        chosen,
        correct: None,
    }
}

fn poll(id: i64, message_id: i64) -> PollSummary {
    PollSummary {
        id,
        chat_id: CHAT_ID,
        message_id,
        question: "Where to lunch?".to_owned(),
        options: vec![
            option("Pizza", Some(2), false),
            option("Soup", Some(1), true),
        ],
        total_voters: Some(3),
        public_voters: true,
        multiple_choice: false,
        quiz: false,
        closed: false,
        explanation: None,
    }
}

fn quiz(id: i64, message_id: i64) -> PollSummary {
    let mut poll = poll(id, message_id);
    poll.quiz = true;
    poll.options = vec![
        PollOption {
            correct: Some(true),
            ..option("Paris", Some(4), false)
        },
        PollOption {
            correct: Some(false),
            ..option("Rome", Some(1), true)
        },
    ];
    poll
}

#[test]
fn valid_single_choice_poll_passes_validation() {
    assert!(poll(1, 10).validate().is_ok());
}

#[test]
fn multiple_choice_poll_may_have_several_chosen_answers() {
    let mut poll = poll(1, 10);
    poll.multiple_choice = true;
    poll.options[0].chosen = true;
    assert!(poll.validate().is_ok());
}

#[test]
fn quiz_with_one_correct_answer_is_valid() {
    assert!(quiz(2, 11).validate().is_ok());
}

#[test]
fn invalid_polls_are_rejected() {
    let mut zero_id = poll(0, 10);
    zero_id.id = 0;

    let mut empty_question = poll(1, 10);
    empty_question.question = "   ".to_owned();

    let mut one_option = poll(1, 10);
    one_option.options.truncate(1);

    let mut too_many = poll(1, 10);
    too_many.options = (0..=MAX_POLL_OPTIONS)
        .map(|index| option(&format!("answer {index}"), None, false))
        .collect();

    let mut empty_answer = poll(1, 10);
    empty_answer.options[0].text = String::new();

    let mut several_chosen_single = poll(1, 10);
    several_chosen_single.options[0].chosen = true;

    let mut quiz_multiple = quiz(2, 11);
    quiz_multiple.multiple_choice = true;

    let mut two_correct = quiz(2, 11);
    two_correct.options[1].correct = Some(true);

    let mut correct_outside_quiz = poll(1, 10);
    correct_outside_quiz.options[0].correct = Some(true);

    for invalid in [
        zero_id,
        empty_question,
        one_option,
        too_many,
        empty_answer,
        several_chosen_single,
        quiz_multiple,
        two_correct,
        correct_outside_quiz,
    ] {
        assert!(invalid.validate().is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn reducer_stores_poll_and_finds_it_by_message() {
    let mut state = CoreState::default();
    let snapshot = poll(1, 10);

    state.reduce(&CoreEvent::PollUpdated(snapshot.clone()));

    assert_eq!(state.poll(1), Some(&snapshot));
    assert_eq!(state.poll_for_message(CHAT_ID, 10), Some(&snapshot));
    assert_eq!(state.poll_for_message(CHAT_ID, 11), None);
    assert_eq!(state.last_error, None);
}

#[test]
fn duplicate_poll_update_is_idempotent() {
    let mut state = CoreState::default();
    let event = CoreEvent::PollUpdated(poll(1, 10));

    state.reduce(&event);
    let after_first = state.clone();
    state.reduce(&event);

    assert_eq!(state, after_first);
}

#[test]
fn updated_results_replace_the_stored_snapshot() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::PollUpdated(poll(1, 10)));

    let mut newer = poll(1, 10);
    newer.options[0].voter_count = Some(5);
    newer.total_voters = Some(6);
    state.reduce(&CoreEvent::PollUpdated(newer.clone()));

    assert_eq!(state.poll(1), Some(&newer));
    assert_eq!(state.polls.len(), 1);
}

#[test]
fn closed_poll_is_not_reopened_by_a_stale_open_snapshot() {
    let mut state = CoreState::default();
    let mut closed = poll(1, 10);
    closed.closed = true;
    state.reduce(&CoreEvent::PollUpdated(closed.clone()));

    state.reduce(&CoreEvent::PollUpdated(poll(1, 10)));

    assert_eq!(state.poll(1), Some(&closed));
}

#[test]
fn closed_poll_accepts_final_results_for_the_same_poll() {
    let mut state = CoreState::default();
    let mut closed = poll(1, 10);
    closed.closed = true;
    state.reduce(&CoreEvent::PollUpdated(closed));

    let mut final_results = poll(1, 10);
    final_results.closed = true;
    final_results.options[0].voter_count = Some(9);
    state.reduce(&CoreEvent::PollUpdated(final_results.clone()));

    assert_eq!(state.poll(1), Some(&final_results));
}

#[test]
fn poll_id_reused_for_another_message_is_rejected() {
    let mut state = CoreState::default();
    let original = poll(1, 10);
    state.reduce(&CoreEvent::PollUpdated(original.clone()));

    state.reduce(&CoreEvent::PollUpdated(poll(1, 99)));

    assert_eq!(state.poll(1), Some(&original));
    assert!(state
        .last_error
        .as_deref()
        .unwrap()
        .contains("different message"));
}

#[test]
fn a_message_cannot_be_assigned_a_second_poll_id() {
    let mut state = CoreState::default();
    let original = poll(1, 10);
    state.reduce(&CoreEvent::PollUpdated(original.clone()));

    state.reduce(&CoreEvent::PollUpdated(poll(2, 10)));

    assert_eq!(state.poll(1), Some(&original));
    assert_eq!(state.poll(2), None);
    assert!(state
        .last_error
        .as_deref()
        .unwrap()
        .contains("already has a different poll"));
}

#[test]
fn invalid_poll_update_sets_error_without_storing() {
    let mut state = CoreState::default();
    let mut invalid = poll(1, 10);
    invalid.question = String::new();

    state.reduce(&CoreEvent::PollUpdated(invalid));

    assert!(state.polls.is_empty());
    assert!(state.last_error.as_deref().unwrap().contains("question"));
}

#[test]
fn deleting_the_message_removes_its_poll() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::PollUpdated(poll(1, 10)));
    state.reduce(&CoreEvent::PollUpdated(poll(2, 11)));

    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: Some(CHAT_ID),
        message_ids: vec![10],
    });

    assert_eq!(state.poll(1), None);
    assert!(state.poll(2).is_some());
    assert_eq!(state.poll_order.iter().copied().collect::<Vec<_>>(), [2]);
    state.validate().unwrap();
}

#[test]
fn poll_cache_evicts_fifo_without_touching_order_on_updates() {
    let mut state = CoreState::default();
    for id in 1..=MAX_CACHED_POLLS as i64 {
        state.reduce(&CoreEvent::PollUpdated(poll(id, id + 1000)));
    }

    let mut updated_oldest = poll(1, 1001);
    updated_oldest.options[0].voter_count = Some(8);
    state.reduce(&CoreEvent::PollUpdated(updated_oldest));
    state.reduce(&CoreEvent::PollUpdated(poll(
        MAX_CACHED_POLLS as i64 + 1,
        9999,
    )));

    assert_eq!(state.polls.len(), MAX_CACHED_POLLS);
    assert_eq!(state.poll_order.len(), MAX_CACHED_POLLS);
    assert_eq!(state.poll_order.front(), Some(&2));
    assert_eq!(
        state.poll_order.back(),
        Some(&(MAX_CACHED_POLLS as i64 + 1))
    );
    assert_eq!(state.poll(1), None);
    assert!(state.poll(2).is_some());
    state.validate().unwrap();

    let encoded = serde_json::to_string(&state).unwrap();
    let restored: CoreState = serde_json::from_str(&encoded).unwrap();
    assert_eq!(restored, state);
}

#[test]
fn poll_update_event_has_stable_wire_kind_and_round_trips() {
    let event = CoreEvent::PollUpdated(quiz(3, 12));

    assert_eq!(event.kind(), "poll_updated");
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "poll_updated");
    let decoded: CoreEvent = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, event);
}

#[test]
fn older_persisted_state_defaults_to_no_polls() {
    let mut json = serde_json::to_value(CoreState::default()).unwrap();
    json.as_object_mut().unwrap().remove("polls");
    json.as_object_mut().unwrap().remove("poll_order");

    let decoded: CoreState = serde_json::from_value(json).unwrap();

    assert!(decoded.polls.is_empty());
}

#[test]
fn vote_selection_validates_single_and_multiple_choice() {
    let single = poll(10, 20);
    assert!(single.validate_vote_selection(&[b"Pizza".to_vec()]).is_ok());
    assert!(single
        .validate_vote_selection(&[b"Pizza".to_vec(), b"Soup".to_vec()])
        .is_err());

    let mut multiple = poll(11, 21);
    multiple.multiple_choice = true;
    assert!(multiple
        .validate_vote_selection(&[b"Pizza".to_vec(), b"Soup".to_vec()])
        .is_ok());
}

#[test]
fn vote_selection_rejects_invalid_option_and_closed_poll() {
    let open_poll = poll(12, 22);
    assert!(open_poll
        .validate_vote_selection(&[b"Unknown".to_vec()])
        .is_err());

    let mut closed = poll(13, 23);
    closed.closed = true;
    assert!(closed
        .validate_vote_selection(&[b"Pizza".to_vec()])
        .is_err());
}

#[test]
fn legacy_poll_without_option_ids_and_explanation_still_deserializes() {
    let mut json = serde_json::to_value(poll(14, 24)).unwrap();
    let object = json.as_object_mut().unwrap();
    object.remove("explanation");
    for option in object["options"].as_array_mut().unwrap() {
        option.as_object_mut().unwrap().remove("option_id");
    }

    let restored: PollSummary = serde_json::from_value(json).unwrap();
    assert!(restored
        .options
        .iter()
        .all(|option| option.option_id.is_empty()));
    assert_eq!(restored.explanation, None);
    assert!(restored.validate().is_ok());
    assert!(restored
        .validate_vote_selection(&[b"Pizza".to_vec()])
        .is_err());
}

#[test]
fn poll_vote_command_has_stable_wire_kind_and_round_trips() {
    let command = CoreCommand::VotePoll {
        poll_id: 99,
        chat_id: CHAT_ID,
        message_id: 10,
        option_ids: vec![b"Pizza".to_vec()],
    };

    assert_eq!(command.kind(), "vote_poll");
    let json = serde_json::to_value(&command).unwrap();
    assert_eq!(json["type"], "vote_poll");
    assert_eq!(json["payload"]["poll_id"], 99);
    let decoded: CoreCommand = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, command);
}
