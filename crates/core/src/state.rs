use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    event::CoreEvent,
    model::{AccountState, ChatSummary, MessageSummary},
    outgoing::{OutgoingMessage, OutgoingMessageStatus},
};

pub const MAX_CACHED_MESSAGES_PER_CHAT: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreState {
    pub running: bool,
    pub account: AccountState,
    pub messages: BTreeMap<i64, Vec<MessageSummary>>,
    #[serde(default)]
    pub outgoing_messages: BTreeMap<String, OutgoingMessage>,
    pub last_error: Option<String>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            running: true,
            account: AccountState::default(),
            messages: BTreeMap::new(),
            outgoing_messages: BTreeMap::new(),
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
            CoreEvent::OutgoingMessageChanged(message) => {
                self.upsert_outgoing_message(message.clone());
            }
            CoreEvent::MessageAdded(message) => {
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
            CoreEvent::Error(error) => {
                self.last_error = Some(error.clone());
            }
            CoreEvent::Stopped => {
                self.running = false;
            }
        }
    }

    pub fn outgoing_message(&self, client_id: &str) -> Option<&OutgoingMessage> {
        self.outgoing_messages.get(client_id)
    }

    /// Converts sends interrupted while the process was down to retryable failures.
    pub(crate) fn fail_interrupted_sends(&mut self) -> Vec<OutgoingMessage> {
        let interrupted = self.outgoing_messages.values()
            .filter(|message| message.status == OutgoingMessageStatus::Sending)
            .map(|message| {
                let mut failed = message.clone();
                failed.status = OutgoingMessageStatus::Failed;
                failed.server_message_id = None;
                failed.error = Some(
                    "send was interrupted before confirmation; retrying may duplicate a message".into(),
                );
                failed
            })
            .collect::<Vec<_>>();
        for message in &interrupted {
            self.upsert_outgoing_message(message.clone());
        }
        interrupted
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

    fn upsert_outgoing_message(&mut self, message: OutgoingMessage) {
        let accepted = match self.outgoing_messages.get(&message.client_id) {
            None => message.status == OutgoingMessageStatus::Draft,
            Some(current) => {
                current.chat_id == message.chat_id
                    && current.text == message.text
                    && (current == &message || current.status.may_transition_to(message.status))
            }
        };
        if accepted {
            self.outgoing_messages.insert(message.client_id.clone(), message);
        }
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


#[cfg(test)]
mod outgoing_message_tests {
    use super::CoreState;
    use crate::{
        event::CoreEvent,
        outgoing::{OutgoingMessage, OutgoingMessageStatus},
    };

    fn message(status: OutgoingMessageStatus, text: &str) -> OutgoingMessage {
        OutgoingMessage {
            client_id: "pending-1".into(),
            chat_id: 7,
            text: text.into(),
            status,
            server_message_id: None,
            error: None,
        }
    }

    #[test]
    fn reducer_accepts_only_valid_identity_preserving_transitions() {
        let mut state = CoreState::default();
        state.reduce(&CoreEvent::OutgoingMessageChanged(message(
            OutgoingMessageStatus::Draft,
            "hello",
        )));
        state.reduce(&CoreEvent::OutgoingMessageChanged(message(
            OutgoingMessageStatus::Sending,
            "different payload",
        )));
        assert_eq!(
            state.outgoing_message("pending-1").unwrap().text,
            "hello",
            "a stable ID cannot be reused for changed text"
        );

        state.reduce(&CoreEvent::OutgoingMessageChanged(message(
            OutgoingMessageStatus::Sending,
            "hello",
        )));
        let mut sent = message(OutgoingMessageStatus::Sent, "hello");
        sent.server_message_id = Some(88);
        state.reduce(&CoreEvent::OutgoingMessageChanged(sent.clone()));
        state.reduce(&CoreEvent::OutgoingMessageChanged(message(
            OutgoingMessageStatus::Failed,
            "hello",
        )));

        let stored = state.outgoing_message("pending-1").unwrap();
        assert_eq!(stored.status, OutgoingMessageStatus::Sent);
        assert_eq!(stored.server_message_id, Some(88));
    }

    #[test]
    fn interrupted_send_recovers_as_failed_with_same_retry_identity() {
        let mut state = CoreState::default();
        state.reduce(&CoreEvent::OutgoingMessageChanged(message(
            OutgoingMessageStatus::Draft,
            "hello",
        )));
        state.reduce(&CoreEvent::OutgoingMessageChanged(message(
            OutgoingMessageStatus::Sending,
            "hello",
        )));

        let recovered = state.fail_interrupted_sends();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].client_id, "pending-1");
        assert_eq!(recovered[0].status, OutgoingMessageStatus::Failed);
        assert!(recovered[0]
            .error
            .as_deref()
            .unwrap()
            .contains("retrying may duplicate"));
        assert_eq!(
            state.outgoing_message("pending-1").unwrap().status,
            OutgoingMessageStatus::Failed
        );
    }
}
