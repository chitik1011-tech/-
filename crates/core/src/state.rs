use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    event::CoreEvent,
    forwarding::ForwardMessagesResult,
    model::{AccountState, ChatSummary, MessageSummary},
};

pub const MAX_CACHED_MESSAGES_PER_CHAT: usize = 512;
pub const MAX_CACHED_FORWARD_REQUESTS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreState {
    pub running: bool,
    pub account: AccountState,
    pub messages: BTreeMap<i64, Vec<MessageSummary>>,
    pub last_error: Option<String>,
    /// Recent correlated outcomes, restored from older snapshots as an empty map.
    #[serde(default)]
    pub forwarding: BTreeMap<String, ForwardMessagesResult>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            running: true,
            account: AccountState::default(),
            messages: BTreeMap::new(),
            last_error: None,
            forwarding: BTreeMap::new(),
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
            CoreEvent::MessagesForwarded(result) => {
                if !self.forwarding.contains_key(&result.request_id) {
                    self.forwarding
                        .insert(result.request_id.clone(), result.clone());
                    while self.forwarding.len() > MAX_CACHED_FORWARD_REQUESTS {
                        if let Some(oldest) = self.forwarding.keys().next().cloned() {
                            self.forwarding.remove(&oldest);
                        }
                    }
                }
                self.last_error = None;
            }
            CoreEvent::Error(error) => self.last_error = Some(error.clone()),
            CoreEvent::Stopped => self.running = false,
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
