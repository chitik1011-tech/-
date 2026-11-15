use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use grammers_client::{media::Media, message::Message, tl};
use tg_mobile_core::{CoreError, MAX_CACHED_POLLS, PollOption, PollSummary};
use tokio::sync::Mutex;

use crate::ids::{PeerClass, encode_peer};

const POLL_REGISTRY_CAPACITY: usize = MAX_CACHED_POLLS;

#[derive(Debug, Default)]
pub(crate) struct PollRegistryState {
    polls: HashMap<i64, PollSummary>,
    order: VecDeque<i64>,
}

/// Protocol-side identity cache used only to resolve partial `updateMessagePoll`
/// updates that omit peer/message identity. CoreState remains the canonical poll
/// state exposed to the application.
pub(crate) type PollRegistry = Arc<Mutex<PollRegistryState>>;

pub(crate) fn new_poll_registry() -> PollRegistry {
    Arc::new(Mutex::new(PollRegistryState::default()))
}

pub(crate) async fn remember_poll(registry: &PollRegistry, poll: PollSummary) {
    let mut state = registry.lock().await;
    if !state.polls.contains_key(&poll.id) {
        state.order.push_back(poll.id);
    }
    state.polls.insert(poll.id, poll);
    while state.order.len() > POLL_REGISTRY_CAPACITY {
        if let Some(oldest) = state.order.pop_front() {
            state.polls.remove(&oldest);
        }
    }
}

pub(crate) async fn poll_by_id(registry: &PollRegistry, poll_id: i64) -> Option<PollSummary> {
    registry.lock().await.polls.get(&poll_id).cloned()
}

pub(crate) async fn forget_deleted_polls(
    registry: &PollRegistry,
    chat_id: Option<i64>,
    message_ids: &[i64],
) {
    let mut state = registry.lock().await;
    state.polls.retain(|_, poll| {
        let chat_matches = chat_id.is_none_or(|chat_id| chat_id == poll.chat_id);
        !(chat_matches && message_ids.contains(&poll.message_id))
    });
    let retained = state
        .polls
        .keys()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    state.order.retain(|poll_id| retained.contains(poll_id));
}

pub(crate) fn poll_from_message(message: &Message) -> tg_mobile_core::Result<Option<PollSummary>> {
    let Some(Media::Poll(poll)) = message.media() else {
        return Ok(None);
    };

    let chat_id = crate::ids::peer_id_to_core(message.peer_id());
    let summary = poll_summary_from_raw(
        chat_id,
        i64::from(message.id()),
        &poll.raw,
        &poll.raw_results,
    )?;
    summary.validate()?;
    Ok(Some(summary))
}

pub(crate) fn poll_summary_from_raw(
    chat_id: i64,
    message_id: i64,
    poll: &tl::types::Poll,
    results: &tl::types::PollResults,
) -> tg_mobile_core::Result<PollSummary> {
    let question = text_with_entities_text(&poll.question).to_owned();
    let options = poll
        .answers
        .iter()
        .filter_map(raw_answer)
        .map(|answer| PollOption {
            text: text_with_entities_text(&answer.text).to_owned(),
            option_id: answer.option.clone(),
            voter_count: None,
            chosen: false,
            correct: None,
        })
        .collect::<Vec<_>>();

    let mut summary = PollSummary {
        id: poll.id,
        chat_id,
        message_id,
        question,
        options,
        total_voters: None,
        public_voters: poll.public_voters,
        multiple_choice: poll.multiple_choice,
        quiz: poll.quiz,
        closed: poll.closed,
        explanation: None,
    };
    apply_results(&mut summary, results, false)?;
    summary.validate()?;
    Ok(summary)
}

pub(crate) async fn poll_from_update(
    update: &tl::types::UpdateMessagePoll,
    registry: &PollRegistry,
) -> tg_mobile_core::Result<Option<PollSummary>> {
    let existing = poll_by_id(registry, update.poll_id).await;

    let explicit_identity = match (&update.peer, update.msg_id) {
        (Some(peer), Some(message_id)) => {
            Some((raw_peer_to_core(peer), i64::from(message_id)))
        }
        _ => None,
    };
    let (chat_id, message_id) = explicit_identity
        .or_else(|| {
            existing
                .as_ref()
                .map(|poll| (poll.chat_id, poll.message_id))
        })
        .ok_or_else(|| {
            CoreError::Backend(format!(
                "poll update {} omitted message identity before the poll was known",
                update.poll_id
            ))
        })?;

    if let Some(existing) = existing.as_ref()
        && (existing.chat_id != chat_id || existing.message_id != message_id)
    {
        return Err(CoreError::Backend(format!(
            "poll {} update points at a different message",
            update.poll_id
        )));
    }

    let existing_closed = existing.as_ref().is_some_and(|poll| poll.closed);
    let results = poll_results(&update.results);
    let mut summary = if let Some(raw_poll) = update.poll.as_ref().map(raw_poll) {
        if raw_poll.id != update.poll_id {
            return Err(CoreError::Backend(
                "poll update id does not match embedded poll".into(),
            ));
        }
        poll_summary_from_raw(chat_id, message_id, raw_poll, results)?
    } else if let Some(existing) = existing {
        existing
    } else {
        return Ok(None);
    };

    apply_results(&mut summary, results, true)?;
    summary.closed |= existing_closed;
    summary.validate()?;
    remember_poll(registry, summary.clone()).await;
    Ok(Some(summary))
}

pub(crate) async fn polls_from_updates(
    updates: &tl::enums::Updates,
    registry: &PollRegistry,
) -> tg_mobile_core::Result<Vec<PollSummary>> {
    let raw_updates: Vec<&tl::enums::Update> = match updates {
        tl::enums::Updates::Updates(batch) => batch.updates.iter().collect(),
        tl::enums::Updates::Combined(batch) => batch.updates.iter().collect(),
        tl::enums::Updates::UpdateShort(short) => vec![&short.update],
        tl::enums::Updates::TooLong
        | tl::enums::Updates::UpdateShortMessage(_)
        | tl::enums::Updates::UpdateShortChatMessage(_)
        | tl::enums::Updates::UpdateShortSentMessage(_) => Vec::new(),
    };

    let mut polls = Vec::new();
    for update in raw_updates {
        if let tl::enums::Update::MessagePoll(update) = update
            && let Some(poll) = poll_from_update(update, registry).await?
        {
            polls.push(poll);
        }
    }
    Ok(polls)
}

fn raw_poll(poll: &tl::enums::Poll) -> &tl::types::Poll {
    match poll {
        tl::enums::Poll::Poll(poll) => poll,
    }
}

fn raw_answer(answer: &tl::enums::PollAnswer) -> Option<&tl::types::PollAnswer> {
    match answer {
        tl::enums::PollAnswer::Answer(answer) => Some(answer),
        tl::enums::PollAnswer::InputPollAnswer(_) => None,
    }
}

fn poll_results(results: &tl::enums::PollResults) -> &tl::types::PollResults {
    match results {
        tl::enums::PollResults::Results(results) => results,
    }
}

fn answer_voters(voters: &tl::enums::PollAnswerVoters) -> &tl::types::PollAnswerVoters {
    match voters {
        tl::enums::PollAnswerVoters::Voters(voters) => voters,
    }
}

fn text_with_entities_text(text: &tl::enums::TextWithEntities) -> &str {
    match text {
        tl::enums::TextWithEntities::Entities(text) => &text.text,
    }
}

fn raw_peer_to_core(peer: &tl::enums::Peer) -> i64 {
    match peer {
        tl::enums::Peer::User(peer) => encode_peer(PeerClass::User, peer.user_id),
        tl::enums::Peer::Chat(peer) => encode_peer(PeerClass::Chat, peer.chat_id),
        tl::enums::Peer::Channel(peer) => encode_peer(PeerClass::Channel, peer.channel_id),
    }
}

fn apply_results(
    summary: &mut PollSummary,
    results: &tl::types::PollResults,
    preserve_partial: bool,
) -> tg_mobile_core::Result<()> {
    if let Some(total_voters) = results.total_voters {
        summary.total_voters = Some(non_negative_u32("poll total voters", total_voters)?);
    } else if !preserve_partial && !results.min {
        summary.total_voters = None;
    }

    if let Some(solution) = results.solution.as_ref() {
        summary.explanation = (!solution.trim().is_empty()).then(|| solution.clone());
    } else if !preserve_partial && !results.min {
        summary.explanation = None;
    }

    let Some(voters) = results.results.as_ref() else {
        return Ok(());
    };

    let reveals_correct = summary.quiz
        && (results.solution.is_some()
            || voters
                .iter()
                .map(answer_voters)
                .any(|voters| voters.correct));

    if !results.min {
        for option in &mut summary.options {
            option.chosen = false;
            option.voter_count = None;
            if summary.quiz {
                option.correct = reveals_correct.then_some(false);
            }
        }
    }

    for voters in voters.iter().map(answer_voters) {
        let Some(option) = summary
            .options
            .iter_mut()
            .find(|option| option.option_id == voters.option)
        else {
            continue;
        };

        option.chosen = voters.chosen;
        if let Some(count) = voters.voters {
            option.voter_count = Some(non_negative_u32("poll option voters", count)?);
        } else if !results.min {
            option.voter_count = None;
        }
        if summary.quiz && reveals_correct {
            option.correct = Some(voters.correct);
        }
    }

    Ok(())
}

fn non_negative_u32(field: &str, value: i32) -> tg_mobile_core::Result<u32> {
    u32::try_from(value)
        .map_err(|_| CoreError::Backend(format!("Telegram returned negative {field}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> tl::enums::TextWithEntities {
        tl::types::TextWithEntities {
            text: value.to_owned(),
            entities: Vec::new(),
        }
        .into()
    }

    fn answer(text_value: &str, option: &[u8]) -> tl::enums::PollAnswer {
        tl::types::PollAnswer {
            text: text(text_value),
            option: option.to_vec(),
            media: None,
            added_by: None,
            date: None,
        }
        .into()
    }

    fn raw_poll(multiple_choice: bool, quiz: bool, closed: bool) -> tl::types::Poll {
        tl::types::Poll {
            id: 99,
            closed,
            public_voters: true,
            multiple_choice,
            quiz,
            open_answers: false,
            revoting_disabled: false,
            shuffle_answers: false,
            hide_results_until_close: false,
            creator: false,
            subscribers_only: false,
            question: text("Capital of France?"),
            answers: vec![answer("Paris", b"a"), answer("Rome", b"b")],
            close_period: None,
            close_date: None,
            countries_iso2: None,
            hash: 0,
        }
    }

    fn voters(
        option: &[u8],
        count: i32,
        chosen: bool,
        correct: bool,
    ) -> tl::enums::PollAnswerVoters {
        tl::types::PollAnswerVoters {
            chosen,
            correct,
            option: option.to_vec(),
            voters: Some(count),
            recent_voters: None,
        }
        .into()
    }

    fn results(
        total: i32,
        answers: Vec<tl::enums::PollAnswerVoters>,
        solution: Option<&str>,
    ) -> tl::types::PollResults {
        tl::types::PollResults {
            min: false,
            has_unread_votes: false,
            can_view_stats: true,
            results: Some(answers),
            total_voters: Some(total),
            recent_voters: None,
            solution: solution.map(str::to_owned),
            solution_entities: None,
            solution_media: None,
        }
    }

    #[test]
    fn maps_regular_poll_and_results_without_inventing_quiz_data() {
        let poll = raw_poll(false, false, false);
        let mapped = poll_summary_from_raw(
            -42,
            7,
            &poll,
            &results(
                3,
                vec![voters(b"a", 2, true, false), voters(b"b", 1, false, false)],
                None,
            ),
        )
        .unwrap();

        assert_eq!(mapped.id, 99);
        assert_eq!(mapped.question, "Capital of France?");
        assert!(mapped.public_voters);
        assert!(!mapped.multiple_choice);
        assert!(!mapped.quiz);
        assert_eq!(mapped.total_voters, Some(3));
        assert_eq!(mapped.options[0].option_id, b"a");
        assert_eq!(mapped.options[0].voter_count, Some(2));
        assert!(mapped.options[0].chosen);
        assert_eq!(mapped.options[0].correct, None);
    }

    #[test]
    fn maps_multiple_choice_closed_poll() {
        let poll = raw_poll(true, false, true);
        let mapped = poll_summary_from_raw(
            -42,
            7,
            &poll,
            &results(
                4,
                vec![voters(b"a", 3, true, false), voters(b"b", 2, true, false)],
                None,
            ),
        )
        .unwrap();

        assert!(mapped.multiple_choice);
        assert!(mapped.closed);
        assert!(mapped.options.iter().all(|option| option.chosen));
    }

    #[test]
    fn maps_quiz_correct_answer_and_solution_only_when_server_reveals_them() {
        let poll = raw_poll(false, true, false);
        let hidden = poll_summary_from_raw(
            -42,
            7,
            &poll,
            &results(
                1,
                vec![voters(b"a", 1, false, false), voters(b"b", 0, false, false)],
                None,
            ),
        )
        .unwrap();
        assert!(hidden.options.iter().all(|option| option.correct.is_none()));
        assert_eq!(hidden.explanation, None);

        let revealed = poll_summary_from_raw(
            -42,
            7,
            &poll,
            &results(
                2,
                vec![voters(b"a", 2, true, true), voters(b"b", 0, false, false)],
                Some("Paris is the capital."),
            ),
        )
        .unwrap();
        assert_eq!(revealed.options[0].correct, Some(true));
        assert_eq!(revealed.options[1].correct, Some(false));
        assert_eq!(
            revealed.explanation.as_deref(),
            Some("Paris is the capital.")
        );
    }

    #[tokio::test]
    async fn duplicate_update_is_stable_and_partial_update_preserves_identity() {
        let registry = new_poll_registry();
        let base = poll_summary_from_raw(
            -42,
            7,
            &raw_poll(false, false, false),
            &results(
                1,
                vec![voters(b"a", 1, false, false), voters(b"b", 0, false, false)],
                None,
            ),
        )
        .unwrap();
        remember_poll(&registry, base).await;

        let update = tl::types::UpdateMessagePoll {
            peer: None,
            msg_id: None,
            top_msg_id: None,
            poll_id: 99,
            poll: None,
            results: results(
                2,
                vec![voters(b"a", 2, true, false), voters(b"b", 0, false, false)],
                None,
            )
            .into(),
        };

        let first = poll_from_update(&update, &registry).await.unwrap().unwrap();
        let second = poll_from_update(&update, &registry).await.unwrap().unwrap();
        assert_eq!(first, second);
        assert_eq!(first.chat_id, -42);
        assert_eq!(first.message_id, 7);
        assert_eq!(first.total_voters, Some(2));
        assert!(first.options[0].chosen);
    }
}
