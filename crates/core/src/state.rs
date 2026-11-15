use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    event::CoreEvent,
    model::{AccountState, ChatSummary, ChatUnreadState, MessageSummary, UnreadTotals},
};

pub const MAX_CACHED_MESSAGES_PER_CHAT: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreState {
    pub running: bool,
    pub account: AccountState,
    pub messages: BTreeMap<i64, Vec<MessageSummary>>,
    /// Unread details keyed by stable chat ID. Missing in old persisted states;
    /// deserialization defaults it to an empty map and reads message counts
    /// from the legacy ChatSummary field.
    #[serde(default)]
    pub unread_by_chat: BTreeMap<i64, ChatUnreadState>,
    pub last_error: Option<String>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            running: true,
            account: AccountState::default(),
            messages: BTreeMap::new(),
            unread_by_chat: BTreeMap::new(),
            last_error: None,
        }
    }
}

impl CoreState {
    pub fn reduce(&mut self, event: &CoreEvent) {
        match event {
            CoreEvent::State(account) => {
                self.account = account.clone();
                self.sync_unread_messages_from_chats();
                self.last_error = None;
            }
            CoreEvent::AuthorizationChanged(authorization) => {
                self.account.authorization = authorization.clone();
                self.last_error = None;
            }
            CoreEvent::ChatsLoaded(chats) => {
                self.account.chats = chats.clone();
                self.sync_unread_messages_from_chats();
                self.last_error = None;
            }
            CoreEvent::MessageAdded(message) => {
                let inserted = self.upsert_message(message.clone());
                let current_unread = self
                    .unread_by_chat
                    .get(&message.chat_id)
                    .map(|state| state.unread_messages)
                    .or_else(|| {
                        self.account
                            .chats
                            .iter()
                            .find(|chat| chat.id == message.chat_id)
                            .map(|chat| chat.unread_count)
                    })
                    .unwrap_or_default();
                let unread_messages = if inserted && !message.outgoing {
                    current_unread.saturating_add(1)
                } else {
                    current_unread
                };

                if let Some(chat) = self
                    .account
                    .chats
                    .iter_mut()
                    .find(|chat| chat.id == message.chat_id)
                {
                    chat.unread_count = unread_messages;
                    chat.last_message = Some(message.clone());
                } else {
                    self.account.chats.push(ChatSummary {
                        id: message.chat_id,
                        title: format!("Chat {}", message.chat_id),
                        unread_count: unread_messages,
                        last_message: Some(message.clone()),
                    });
                }

                self.unread_by_chat
                    .entry(message.chat_id)
                    .or_default()
                    .unread_messages = unread_messages;
                self.last_error = None;
            }
            CoreEvent::ChatUnreadUpdated(update) => {
                self.apply_unread_update(update);
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

    pub fn chat(&self, chat_id: i64) -> Option<&ChatSummary> {
        self.account.chats.iter().find(|chat| chat.id == chat_id)
    }

    pub fn messages_for_chat(&self, chat_id: i64) -> &[MessageSummary] {
        self.messages
            .get(&chat_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Returns the best known counters for a chat, including legacy message
    /// counts when loading a CoreState written before the unread map existed.
    pub fn unread_state_for_chat(&self, chat_id: i64) -> Option<ChatUnreadState> {
        self.unread_by_chat.get(&chat_id).cloned().or_else(|| {
            self.chat(chat_id).map(|chat| ChatUnreadState {
                unread_messages: chat.unread_count,
                ..ChatUnreadState::default()
            })
        })
    }

    /// Computes aggregate message, mention, reaction, mute, and unread-mark
    /// counts. The app badge counts messages only to avoid double-counting a
    /// message that also contributes to mention/reaction counters.
    pub fn unread_totals(&self) -> UnreadTotals {
        let mut totals = UnreadTotals::default();
        let mut seen_chats = BTreeSet::new();

        for chat in &self.account.chats {
            seen_chats.insert(chat.id);
            let unread = self
                .unread_by_chat
                .get(&chat.id)
                .cloned()
                .unwrap_or_else(|| ChatUnreadState {
                    unread_messages: chat.unread_count,
                    ..ChatUnreadState::default()
                });
            add_unread_totals(&mut totals, &unread);
        }

        for (chat_id, unread) in &self.unread_by_chat {
            if !seen_chats.contains(chat_id) {
                add_unread_totals(&mut totals, unread);
            }
        }

        totals.badge_count = totals.unread_messages;
        totals
    }

    pub fn total_unread_count(&self) -> u64 {
        self.unread_totals().unread_messages
    }

    fn sync_unread_messages_from_chats(&mut self) {
        for chat in &self.account.chats {
            self.unread_by_chat
                .entry(chat.id)
                .or_default()
                .unread_messages = chat.unread_count;
        }
    }

    fn apply_unread_update(&mut self, update: &crate::model::ChatUnreadUpdate) {
        if update.chat_id == 0 {
            return;
        }

        let initial_messages = self
            .account
            .chats
            .iter()
            .find(|chat| chat.id == update.chat_id)
            .map(|chat| chat.unread_count)
            .unwrap_or_default();
        let unread = self
            .unread_by_chat
            .entry(update.chat_id)
            .or_insert_with(|| ChatUnreadState {
                unread_messages: initial_messages,
                ..ChatUnreadState::default()
            });

        if let Some(value) = update.unread_messages {
            unread.unread_messages = value;
        }
        if let Some(value) = update.unread_mentions {
            unread.unread_mentions = value;
        }
        if let Some(value) = update.unread_reactions {
            unread.unread_reactions = value;
        }
        if let Some(value) = update.muted {
            unread.muted = value;
        }
        if let Some(value) = update.unread_mark {
            unread.unread_mark = value;
        }

        let unread_messages = unread.unread_messages;
        if let Some(chat) = self
            .account
            .chats
            .iter_mut()
            .find(|chat| chat.id == update.chat_id)
        {
            chat.unread_count = unread_messages;
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

fn add_unread_totals(totals: &mut UnreadTotals, unread: &ChatUnreadState) {
    totals.unread_messages = totals
        .unread_messages
        .saturating_add(u64::from(unread.unread_messages));
    totals.unread_mentions = totals
        .unread_mentions
        .saturating_add(u64::from(unread.unread_mentions));
    totals.unread_reactions = totals
        .unread_reactions
        .saturating_add(u64::from(unread.unread_reactions));
    totals.muted_chats = totals
        .muted_chats
        .saturating_add(if unread.muted { 1 } else { 0 });
    totals.marked_unread_chats = totals
        .marked_unread_chats
        .saturating_add(if unread.unread_mark { 1 } else { 0 });

    if unread.unread_messages > 0
        || unread.unread_mentions > 0
        || unread.unread_reactions > 0
        || unread.unread_mark
    {
        totals.unread_chats = totals.unread_chats.saturating_add(1);
    }
}
