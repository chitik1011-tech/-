use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    event::CoreEvent,
    model::{AccountState, ChatSummary, MessageSummary},
    send::{OutgoingSend, OutgoingSendState},
};

pub const MAX_CACHED_MESSAGES_PER_CHAT: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreState {
    pub running: bool,
    pub account: AccountState,
    pub messages: BTreeMap<i64, Vec<MessageSummary>>,
    /// Durable reliable-send records. Sent records remain here to prevent client_id reuse.
    /// Older persisted state did not contain this field.
    #[serde(default)]
    pub outbox: BTreeMap<i64, OutgoingSend>,
    pub last_error: Option<String>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            running: true,
            account: AccountState::default(),
            messages: BTreeMap::new(),
            outbox: BTreeMap::new(),
            last_error: None,
        }
    }
}

impl CoreState {
    pub(crate) fn begin_reliable_send(
        &mut self,
        client_id: i64,
        chat_id: i64,
        text: String,
    ) -> crate::error::Result<bool> {
        if let Some(existing) = self.outbox.get(&client_id) {
            if !existing.same_request(chat_id, &text) {
                return Err(crate::error::CoreError::InvalidInput(format!(
                    "client message id {client_id} was reused for a different outgoing message"
                )));
            }

            match &existing.state {
                OutgoingSendState::Sent { .. } => return Ok(false),
                OutgoingSendState::Failed {
                    retryable: false, ..
                } => {
                    return Err(crate::error::CoreError::InvalidInput(format!(
                        "client message id {client_id} has a permanent send failure"
                    )));
                }
                _ => {}
            }
        }

        self.outbox
            .insert(client_id, OutgoingSend::sending(client_id, chat_id, text));
        Ok(true)
    }

    pub(crate) fn recover_inflight_sends(&mut self) -> bool {
        let mut changed = false;
        for send in self.outbox.values_mut() {
            if matches!(&send.state, OutgoingSendState::Sending) {
                send.state = OutgoingSendState::AwaitingConfirmation {
                    reason: "previous process stopped before send confirmation".into(),
                };
                changed = true;
            }
        }
        changed
    }

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
            CoreEvent::MessagesLoaded { chat_id, messages } => {
                for message in messages {
                    if message.chat_id == *chat_id {
                        self.upsert_message(message.clone());
                    }
                }

                if let Some(latest) = self
                    .messages_for_chat(*chat_id)
                    .iter()
                    .max_by_key(|message| (message.timestamp_unix, message.id))
                    .cloned()
                {
                    if let Some(chat) = self
                        .account
                        .chats
                        .iter_mut()
                        .find(|chat| chat.id == *chat_id)
                    {
                        let replace = chat
                            .last_message
                            .as_ref()
                            .map(|current| {
                                (latest.timestamp_unix, latest.id)
                                    > (current.timestamp_unix, current.id)
                            })
                            .unwrap_or(true);
                        if replace {
                            chat.last_message = Some(latest);
                        }
                    }
                }

                self.last_error = None;
            }
            CoreEvent::ChatRead {
                chat_id,
                max_message_id,
            } => {
                if let Some(chat) = self
                    .account
                    .chats
                    .iter_mut()
                    .find(|chat| chat.id == *chat_id)
                {
                    let covers_latest = chat
                        .last_message
                        .as_ref()
                        .map(|message| message.id <= *max_message_id)
                        .unwrap_or(true);
                    if covers_latest {
                        chat.unread_count = 0;
                    }
                }
                self.last_error = None;
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
            CoreEvent::MessageSendStateChanged(update) => {
                let client_id = update.client_id;
                if let Some(existing) = self.outbox.get(&client_id) {
                    if !existing.same_request(update.chat_id, &update.text) {
                        self.last_error = Some(format!(
                            "client message id {client_id} was reused for a different outgoing message"
                        ));
                        return;
                    }
                }

                if let Some(existing) = self.outbox.get(&client_id) {
                    if let OutgoingSendState::Sent {
                        message_id: existing_message_id,
                    } = &existing.state
                    {
                        match &update.state {
                            OutgoingSendState::Sent { message_id }
                                if message_id == existing_message_id => {}
                            OutgoingSendState::Sent { .. } => {
                                self.last_error = Some(format!(
                                    "conflicting acknowledgements for client message id {client_id}"
                                ));
                            }
                            _ => {}
                        }
                        return;
                    }
                }

                match update.state {
                    OutgoingSendState::Sent { .. }
                    | OutgoingSendState::Sending
                    | OutgoingSendState::AwaitingConfirmation { .. }
                    | OutgoingSendState::Failed { .. } => {
                        self.outbox.insert(client_id, update.clone());
                    }
                }
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

    pub fn outgoing_send(&self, client_id: i64) -> Option<&OutgoingSend> {
        self.outbox.get(&client_id)
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

#[cfg(test)]
mod tests {
    use crate::{event::CoreEvent, send::OutgoingSend};

    use super::CoreState;

    #[test]
    fn recovery_moves_inflight_sends_to_awaiting_confirmation() {
        let mut state = CoreState::default();
        state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
            9, 2, "hello",
        )));

        assert!(state.recover_inflight_sends());
        assert!(matches!(
            state.outgoing_send(9).map(|send| &send.state),
            Some(crate::send::OutgoingSendState::AwaitingConfirmation { .. })
        ));
        assert!(!state.recover_inflight_sends());
    }

    #[test]
    fn duplicate_acknowledgement_keeps_one_stable_sent_record() {
        let mut state = CoreState::default();
        let acknowledgement =
            CoreEvent::MessageSendStateChanged(OutgoingSend::sent(9, 2, "hello", 55));
        state.reduce(&acknowledgement);
        state.reduce(&acknowledgement);

        assert_eq!(state.outbox.len(), 1);
        assert!(matches!(
            state.outgoing_send(9).map(|send| &send.state),
            Some(crate::send::OutgoingSendState::Sent { message_id: 55 })
        ));
    }

    #[test]
    fn conflicting_duplicate_acknowledgement_does_not_replace_first_id() {
        let mut state = CoreState::default();
        state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sent(
            9, 2, "hello", 55,
        )));
        state.reduce(&CoreEvent::MessageSendStateChanged(OutgoingSend::sent(
            9, 2, "hello", 56,
        )));

        assert!(matches!(
            state.outgoing_send(9).map(|send| &send.state),
            Some(crate::send::OutgoingSendState::Sent { message_id: 55 })
        ));
        assert!(state
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("conflicting acknowledgements")));
    }
}
