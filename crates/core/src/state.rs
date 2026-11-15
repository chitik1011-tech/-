use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::{
    event::CoreEvent,
    model::{AccountState, ChatSummary, MessageReaction, MessageSummary},
};

pub const MAX_CACHED_MESSAGES_PER_CHAT: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreState {
    pub running: bool,
    pub account: AccountState,
    pub messages: BTreeMap<i64, Vec<MessageSummary>>,
    pub last_error: Option<String>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            running: true,
            account: AccountState::default(),
            messages: BTreeMap::new(),
            last_error: None,
        }
    }
}

impl CoreState {
    pub fn reduce(&mut self, event: &CoreEvent) {
        match event {
            CoreEvent::State(account) => {
                self.account = account.clone();
                self.last_error = None;
            }
            CoreEvent::AuthorizationChanged(authorization) => {
                self.account.authorization = authorization.clone();
                self.last_error = None;
            }
            CoreEvent::ChatsLoaded(chats) => {
                self.account.chats = chats.clone();
                self.last_error = None;
            }
            CoreEvent::MessageAdded(message) => {
                let mut message = message.clone();
                if message.reactions.is_empty() {
                    if let Some(previous) = self
                        .messages
                        .get(&message.chat_id)
                        .and_then(|messages| messages.iter().find(|item| item.id == message.id))
                    {
                        message.reactions = previous.reactions.clone();
                    }
                }
                let inserted = self.upsert_message(message.clone());

                if let Some(chat) = self
                    .account
                    .chats
                    .iter_mut()
                    .find(|chat| chat.id == message.chat_id)
                {
                    if inserted && !message.outgoing {
                        chat.unread_count = chat.unread_count.saturating_add(1);
                    }
                    chat.last_message = Some(message.clone());
                } else {
                    self.account.chats.push(ChatSummary {
                        id: message.chat_id,
                        title: format!("Chat {}", message.chat_id),
                        unread_count: if message.outgoing { 0 } else { 1 },
                        last_message: Some(message.clone()),
                    });
                }

                self.last_error = None;
            }
            CoreEvent::MessageReactionsUpdated {
                chat_id,
                message_id,
                reactions,
            } => {
                let reactions = canonical_reactions(reactions);
                if let Some(messages) = self.messages.get_mut(chat_id) {
                    if let Some(message) = messages
                        .iter_mut()
                        .find(|message| message.id == *message_id)
                    {
                        message.reactions = reactions.clone();
                    }
                }
                if let Some(message) = self
                    .account
                    .chats
                    .iter_mut()
                    .filter(|chat| chat.id == *chat_id)
                    .filter_map(|chat| chat.last_message.as_mut())
                    .find(|message| message.id == *message_id)
                {
                    message.reactions = reactions;
                }
                self.last_error = None;
            }
            CoreEvent::ReactionOperationSucceeded { .. } => {
                self.last_error = None;
            }
            CoreEvent::ReactionOperationFailed { error, .. } => {
                self.last_error = Some(error.clone());
            }
            CoreEvent::Error(error) => {
                self.last_error = Some(error.clone());
            }
            CoreEvent::Stopped => {
                self.running = false;
            }
        }
    }

    pub fn chat(&self, chat_id: i64) -> Option<&ChatSummary> {
        self.account.chats.iter().find(|chat| chat.id == chat_id)
    }

    pub fn messages_for_chat(&self, chat_id: i64) -> &[MessageSummary] {
        self.messages
            .get(&chat_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn total_unread_count(&self) -> u64 {
        self.account
            .chats
            .iter()
            .map(|chat| u64::from(chat.unread_count))
            .sum()
    }

    fn upsert_message(&mut self, message: MessageSummary) -> bool {
        let messages = self.messages.entry(message.chat_id).or_default();
        let inserted = if let Some(index) = messages.iter().position(|item| item.id == message.id) {
            messages[index] = message;
            false
        } else {
            messages.push(message);
            true
        };

        messages.sort_by_key(|item| (item.timestamp_unix, item.id));

        let excess = messages.len().saturating_sub(MAX_CACHED_MESSAGES_PER_CHAT);
        if excess > 0 {
            messages.drain(..excess);
        }

        inserted
    }
}

/// Merge duplicate emoji entries deterministically and discard empty reactions.
fn canonical_reactions(reactions: &[MessageReaction]) -> Vec<MessageReaction> {
    let mut merged: HashMap<&str, (u32, bool)> = HashMap::new();
    for reaction in reactions {
        if reaction.emoji.trim().is_empty() || reaction.count == 0 {
            continue;
        }
        let entry = merged.entry(reaction.emoji.as_str()).or_insert((0, false));
        entry.0 = entry.0.saturating_add(reaction.count);
        entry.1 |= reaction.chosen_by_current_user;
    }

    let mut result: Vec<_> = merged
        .into_iter()
        .map(|(emoji, (count, chosen_by_current_user))| MessageReaction {
            emoji: emoji.to_owned(),
            count,
            chosen_by_current_user,
        })
        .collect();
    result.sort_by(|left, right| left.emoji.cmp(&right.emoji));
    result
}

#[cfg(test)]
mod reaction_tests {
    use super::*;
    use crate::model::{AuthorizationState, ReactionOperation};

    fn message(chat_id: i64, id: i64) -> MessageSummary {
        MessageSummary {
            id,
            chat_id,
            sender_id: 7,
            text: "hello".into(),
            timestamp_unix: 1,
            outgoing: false,
            reactions: Vec::new(),
        }
    }

    fn reaction(emoji: &str, count: u32, chosen: bool) -> MessageReaction {
        MessageReaction {
            emoji: emoji.into(),
            count,
            chosen_by_current_user: chosen,
        }
    }

    fn add_message(state: &mut CoreState, chat_id: i64, id: i64) {
        state.reduce(&CoreEvent::MessageAdded(message(chat_id, id)));
    }

    #[test]
    fn reaction_update_is_isolated_to_addressed_chat_and_message() {
        let mut state = CoreState::default();
        add_message(&mut state, 1, 10);
        add_message(&mut state, 2, 10);
        add_message(&mut state, 1, 11);
        let update = CoreEvent::MessageReactionsUpdated {
            chat_id: 1,
            message_id: 10,
            reactions: vec![reaction("👍", 2, true)],
        };
        state.reduce(&update);
        state.reduce(&update);
        assert_eq!(
            state.messages_for_chat(1)[0].reactions,
            vec![reaction("👍", 2, true)]
        );
        assert!(state.messages_for_chat(2)[0].reactions.is_empty());
        assert!(state.messages_for_chat(1)[1].reactions.is_empty());
        assert_eq!(
            state
                .chat(1)
                .and_then(|chat| chat.last_message.as_ref())
                .unwrap()
                .id,
            11
        );
    }

    #[test]
    fn absolute_updates_add_replace_and_remove_reactions_idempotently() {
        let mut state = CoreState::default();
        add_message(&mut state, 1, 10);
        let add = CoreEvent::MessageReactionsUpdated {
            chat_id: 1,
            message_id: 10,
            reactions: vec![reaction("❤️", 1, true), reaction("🔥", 3, false)],
        };
        state.reduce(&add);
        state.reduce(&add);
        assert_eq!(state.messages_for_chat(1)[0].reactions.len(), 2);
        state.reduce(&CoreEvent::MessageReactionsUpdated {
            chat_id: 1,
            message_id: 10,
            reactions: vec![reaction("🔥", 2, false)],
        });
        assert_eq!(
            state.messages_for_chat(1)[0].reactions,
            vec![reaction("🔥", 2, false)]
        );
        state.reduce(&CoreEvent::MessageReactionsUpdated {
            chat_id: 1,
            message_id: 10,
            reactions: vec![],
        });
        assert!(state.messages_for_chat(1)[0].reactions.is_empty());
    }

    #[test]
    fn duplicate_emoji_entries_merge_and_update_chat_last_message_copy() {
        let mut state = CoreState::default();
        add_message(&mut state, 1, 10);
        let update = CoreEvent::MessageReactionsUpdated {
            chat_id: 1,
            message_id: 10,
            reactions: vec![reaction("👍", 2, false), reaction("👍", 1, true)],
        };
        state.reduce(&update);
        let expected = vec![reaction("👍", 3, true)];
        assert_eq!(state.messages_for_chat(1)[0].reactions, expected);
        assert_eq!(
            state
                .chat(1)
                .unwrap()
                .last_message
                .as_ref()
                .unwrap()
                .reactions,
            expected
        );
        state.reduce(&CoreEvent::ReactionOperationSucceeded {
            chat_id: 1,
            message_id: 10,
            emoji: "👍".into(),
            operation: ReactionOperation::Add,
        });
        assert_eq!(state.messages_for_chat(1)[0].reactions, expected);
    }

    #[test]
    fn duplicate_message_updates_preserve_the_current_reaction_snapshot() {
        let mut state = CoreState::default();
        add_message(&mut state, 1, 10);
        state.reduce(&CoreEvent::MessageReactionsUpdated {
            chat_id: 1,
            message_id: 10,
            reactions: vec![reaction("👍", 2, true)],
        });

        let mut updated = message(1, 10);
        updated.text = "edited text".into();
        state.reduce(&CoreEvent::MessageAdded(updated));

        assert_eq!(
            state.messages_for_chat(1)[0].reactions,
            vec![reaction("👍", 2, true)]
        );
        assert_eq!(
            state.chat(1).unwrap().last_message.as_ref().unwrap().reactions,
            vec![reaction("👍", 2, true)]
        );
    }

    #[test]
    fn old_message_json_defaults_to_empty_reactions_and_new_wire_round_trips() {
        let old = r#"{"id":10,"chat_id":1,"sender_id":7,"text":"hello","timestamp_unix":1,"outgoing":false}"#;
        let parsed: MessageSummary = serde_json::from_str(old).unwrap();
        assert!(parsed.reactions.is_empty());
        let event = CoreEvent::MessageReactionsUpdated {
            chat_id: 1,
            message_id: 10,
            reactions: vec![reaction("👍", 4, true)],
        };
        let wire = serde_json::to_string(&event).unwrap();
        let decoded: CoreEvent = serde_json::from_str(&wire).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn commands_have_stable_wire_names_and_validate_reaction_targets() {

        use crate::command::MAX_REACTION_EMOJI_BYTES;
        use crate::{command::CoreCommand, error::CoreError};
        let valid = CoreCommand::AddReaction {
            chat_id: -1,
            message_id: 2,
            emoji: "🫶".into(),
        };
        assert!(valid.validate().is_ok());
        assert_eq!(valid.kind(), "add_reaction");
        assert_eq!(
            CoreCommand::RemoveReaction {
                chat_id: 1,
                message_id: 2,
                emoji: "👍".into()
            }
            .kind(),
            "remove_reaction"
        );
        for invalid in [
            CoreCommand::AddReaction {
                chat_id: 0,
                message_id: 2,
                emoji: "👍".into(),
            },
            CoreCommand::AddReaction {
                chat_id: 1,
                message_id: 0,
                emoji: "👍".into(),
            },
            CoreCommand::RemoveReaction {
                chat_id: 1,
                message_id: 2,
                emoji: "  ".into(),
            },
            CoreCommand::RemoveReaction {
                chat_id: 1,
                message_id: 2,
                emoji: "é".repeat(MAX_REACTION_EMOJI_BYTES).into(),
            },
        ] {
            assert!(matches!(
                invalid.validate(),
                Err(CoreError::InvalidInput(_))
            ));
        }
    }

    #[test]
    fn event_wire_includes_operation_acknowledgement_and_failure() {
        let events = [
            CoreEvent::ReactionOperationSucceeded {
                chat_id: 1,
                message_id: 10,
                emoji: "👍".into(),
                operation: ReactionOperation::Add,
            },
            CoreEvent::ReactionOperationFailed {
                chat_id: 1,
                message_id: 10,
                emoji: "👍".into(),
                operation: ReactionOperation::Remove,
                error: "failed".into(),
            },
        ];
        let success = serde_json::to_string(&events[0]).unwrap();
        let failure = serde_json::to_string(&events[1]).unwrap();
        assert!(success.contains("reaction_operation_succeeded"));
        assert!(failure.contains("reaction_operation_failed"));
        assert_eq!(
            serde_json::from_str::<CoreEvent>(&success).unwrap(),
            events[0]
        );
        assert_eq!(
            serde_json::from_str::<CoreEvent>(&failure).unwrap(),
            events[1]
        );
    }

    #[test]
    fn reaction_update_for_unknown_message_does_not_create_chat_or_message() {
        let mut state = CoreState::default();
        state.reduce(&CoreEvent::MessageReactionsUpdated {
            chat_id: 777,
            message_id: 42,
            reactions: vec![reaction("👍", 1, true)],
        });
        assert!(state.messages_for_chat(777).is_empty());
        assert!(state.chat(777).is_none());
        assert_eq!(state.account.authorization, AuthorizationState::Closed);
    }
}
