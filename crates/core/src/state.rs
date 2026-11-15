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
    /// Reliable sends that are still in flight or failed and can be retried.
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

                match &update.state {
                    OutgoingSendState::Sent { .. } => {
                        self.outbox.remove(&client_id);
                    }
                    OutgoingSendState::Sending | OutgoingSendState::Failed { .. } => {
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
