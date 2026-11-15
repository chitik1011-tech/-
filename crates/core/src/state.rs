use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    event::CoreEvent,
    model::{AccountState, ChatPage, ChatPaginationState, ChatSummary, MessageSummary},
};

pub const MAX_CACHED_MESSAGES_PER_CHAT: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreState {
    pub running: bool,
    pub account: AccountState,
    pub messages: BTreeMap<i64, Vec<MessageSummary>>,
    /// Cursor progress for the current sequence of chat pages.
    #[serde(default)]
    pub chat_pagination: ChatPaginationState,
    pub last_error: Option<String>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            running: true,
            account: AccountState::default(),
            messages: BTreeMap::new(),
            chat_pagination: ChatPaginationState::default(),
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
                self.chat_pagination = ChatPaginationState::default();
                self.last_error = None;
            }
            CoreEvent::ChatsPageLoading { cursor } => {
                self.chat_pagination.loading = true;
                self.chat_pagination.loading_cursor = cursor.clone();
                self.chat_pagination.error = None;
                self.last_error = None;
            }
            CoreEvent::ChatsPageLoaded(page) => {
                if self.can_accept_chat_page(page) && self.merge_chat_page(page) {
                    self.chat_pagination.loading = false;
                    self.chat_pagination.loading_cursor = None;
                    self.chat_pagination.error = None;
                    self.last_error = None;
                }
            }
            CoreEvent::ChatsPageFailed { cursor, error } => {
                if self.chat_pagination.loading
                    && self.chat_pagination.loading_cursor.as_ref() == cursor.as_ref()
                {
                    self.chat_pagination.loading = false;
                    self.chat_pagination.loading_cursor = None;
                    self.chat_pagination.error = Some(error.clone());
                    self.last_error = Some(error.clone());
                }
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

    fn can_accept_chat_page(&self, page: &ChatPage) -> bool {
        self.chat_pagination.loading && self.chat_pagination.loading_cursor == page.requested_cursor
    }

    fn merge_chat_page(&mut self, page: &ChatPage) -> bool {
        let first_page = page.requested_cursor.is_none();

        if let Some(requested_cursor) = page.requested_cursor.as_deref() {
            if !self.chat_pagination.loaded
                || self.chat_pagination.next_cursor.as_deref() != Some(requested_cursor)
                || self
                    .chat_pagination
                    .consumed_cursors
                    .contains(requested_cursor)
            {
                return false;
            }
        }

        let previous_chats = if first_page {
            std::mem::take(&mut self.account.chats)
        } else {
            Vec::new()
        };

        if first_page {
            let loading = self.chat_pagination.loading;
            self.chat_pagination = ChatPaginationState::default();
            self.chat_pagination.loading = loading;
        } else if let Some(requested_cursor) = page.requested_cursor.as_ref() {
            self.chat_pagination
                .consumed_cursors
                .insert(requested_cursor.clone());
        }

        for chat in page.chats.iter().cloned() {
            self.upsert_chat_summary(chat);
        }

        if first_page {
            // A live MessageAdded can arrive while the first page is in flight.
            // Retain those live-only rows after replacing the previous page set.
            for chat in previous_chats {
                if self.messages.contains_key(&chat.id) {
                    self.upsert_chat_summary(chat);
                }
            }
            self.chat_pagination.loaded = true;
        }

        let next_cursor = page
            .next_cursor
            .as_ref()
            .filter(|cursor| !cursor.trim().is_empty())
            .cloned();
        let cursor_was_consumed = next_cursor
            .as_ref()
            .is_some_and(|cursor| self.chat_pagination.consumed_cursors.contains(cursor));
        self.chat_pagination.next_cursor = if cursor_was_consumed {
            None
        } else {
            next_cursor
        };

        true
    }

    fn upsert_chat_summary(&mut self, incoming: ChatSummary) {
        if let Some(existing) = self
            .account
            .chats
            .iter_mut()
            .find(|chat| chat.id == incoming.id)
        {
            merge_chat_summary(existing, incoming);
        } else {
            self.account.chats.push(incoming);
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

fn merge_chat_summary(existing: &mut ChatSummary, incoming: ChatSummary) {
    let incoming_is_placeholder = incoming.title == format!("Chat {}", incoming.id);
    let existing_is_placeholder = existing.title == format!("Chat {}", existing.id);
    if !incoming.title.trim().is_empty()
        && (!incoming_is_placeholder || existing.title.trim().is_empty() || existing_is_placeholder)
    {
        existing.title = incoming.title;
    }

    existing.unread_count = existing.unread_count.max(incoming.unread_count);
    existing.last_message = match (existing.last_message.take(), incoming.last_message) {
        (Some(current), Some(next))
            if (current.timestamp_unix, current.id) >= (next.timestamp_unix, next.id) =>
        {
            Some(current)
        }
        (_, next) => next,
    };
}
