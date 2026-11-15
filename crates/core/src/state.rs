use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    event::CoreEvent,
    model::{AccountState, ChatSummary, MessageCursor, MessageHistoryState, MessageSummary},
};

pub const MAX_CACHED_MESSAGES_PER_CHAT: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreState {
    pub running: bool,
    pub account: AccountState,
    pub messages: BTreeMap<i64, Vec<MessageSummary>>,
    #[serde(default)]
    pub message_history: BTreeMap<i64, MessageHistoryState>,
    pub last_error: Option<String>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            running: true,
            account: AccountState::default(),
            messages: BTreeMap::new(),
            message_history: BTreeMap::new(),
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
            CoreEvent::MessagesLoading { chat_id, .. } => {
                let history = self.message_history.entry(*chat_id).or_default();
                history.loading = true;
                history.error = None;
            }
            CoreEvent::MessagesLoaded {
                chat_id,
                cursor,
                messages,
                next_cursor,
                has_more,
            } => {
                self.merge_message_page(*chat_id, cursor.as_ref(), messages);

                let page_has_messages = messages.iter().any(|message| message.chat_id == *chat_id);
                let cursor_advanced = next_cursor.as_ref() != cursor.as_ref();
                let has_more = *has_more
                    && page_has_messages
                    && cursor_advanced
                    && next_cursor.is_some();
                let history = self.message_history.entry(*chat_id).or_default();
                history.loading = false;
                history.has_more = has_more;
                history.next_cursor = if has_more { next_cursor.clone() } else { None };
                history.error = None;
                self.last_error = None;
            }
            CoreEvent::MessageHistoryLoadFailed { chat_id, error, .. } => {
                let history = self.message_history.entry(*chat_id).or_default();
                history.loading = false;
                history.error = Some(error.clone());
                self.last_error = Some(error.clone());
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

    pub fn chat(&self, chat_id: i64) -> Option<&ChatSummary> {
        self.account.chats.iter().find(|chat| chat.id == chat_id)
    }

    pub fn messages_for_chat(&self, chat_id: i64) -> &[MessageSummary] {
        self.messages
            .get(&chat_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn message_history_for_chat(&self, chat_id: i64) -> MessageHistoryState {
        self.message_history.get(&chat_id).cloned().unwrap_or_default()
    }

    pub fn total_unread_count(&self) -> u64 {
        self.account
            .chats
            .iter()
            .map(|chat| u64::from(chat.unread_count))
            .sum()
    }

    fn merge_message_page(
        &mut self,
        chat_id: i64,
        cursor: Option<&MessageCursor>,
        incoming: &[MessageSummary],
    ) {
        let mut by_id = BTreeMap::<i64, MessageSummary>::new();
        if let Some(existing) = self.messages.get(&chat_id) {
            for message in existing.iter().filter(|message| message.chat_id == chat_id) {
                by_id.insert(message.id, message.clone());
            }
        }
        for message in incoming.iter().filter(|message| message.chat_id == chat_id) {
            by_id.insert(message.id, message.clone());
        }

        let mut merged: Vec<_> = by_id.into_values().collect();
        merged.sort_by_key(|message| (message.timestamp_unix, message.id));
        if merged.len() > MAX_CACHED_MESSAGES_PER_CHAT {
            let excess = merged.len() - MAX_CACHED_MESSAGES_PER_CHAT;
            if cursor.is_some() {
                // Older pages are prepended; preserve that visible history window.
                merged.truncate(MAX_CACHED_MESSAGES_PER_CHAT);
            } else {
                // The first page is newest-first from the backend; retain recent messages.
                merged.drain(..excess);
            }
        }
        self.messages.insert(chat_id, merged);
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
