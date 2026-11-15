use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    connection::ConnectionState,
    event::CoreEvent,
    model::{AccountState, ChatSummary, MessageHistoryState, MessageSummary},
};

pub const MAX_CACHED_MESSAGES_PER_CHAT: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreState {
    pub running: bool,
    pub account: AccountState,
    /// Current Telegram transport state. Older persisted snapshots default to disconnected.
    #[serde(default)]
    pub connection_state: ConnectionState,
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
            connection_state: ConnectionState::Disconnected,
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
            CoreEvent::ConnectionStateChanged(connection_state) => {
                self.connection_state = *connection_state;
            }
            CoreEvent::ChatsLoaded(chats) => {
                self.account.chats = chats.clone();
                self.last_error = None;
            }
            CoreEvent::MessagesLoaded { chat_id, messages } => {
                self.merge_messages(*chat_id, messages);
                self.last_error = None;
            }
            CoreEvent::MessagesPageLoading {
                chat_id,
                before_message_id,
            } => {
                let history = self.message_history.entry(*chat_id).or_default();
                history.loading = true;
                history.requested_before_message_id = *before_message_id;
                history.error = None;
            }
            CoreEvent::MessagesPageLoaded {
                chat_id,
                before_message_id,
                messages,
                next_before_message_id,
                has_more,
            } => {
                let accepts_page = self.message_history.get(chat_id).is_some_and(|history| {
                    history.loading
                        && history.requested_before_message_id == *before_message_id
                });
                if !accepts_page {
                    return;
                }

                self.merge_messages(*chat_id, messages);

                let next_cursor = next_before_message_id
                    .filter(|message_id| *message_id > 0)
                    .filter(|message_id| Some(*message_id) != *before_message_id);
                let history = self.message_history.entry(*chat_id).or_default();
                history.loading = false;
                history.requested_before_message_id = None;
                history.has_more = *has_more && next_cursor.is_some();
                history.next_before_message_id = history.has_more.then_some(next_cursor).flatten();
                history.error = None;
                self.last_error = None;
            }
            CoreEvent::MessagesPageLoadFailed {
                chat_id,
                before_message_id,
                error,
            } => {
                let accepts_failure =
                    self.message_history.get(chat_id).is_some_and(|history| {
                        history.loading
                            && history.requested_before_message_id == *before_message_id
                    });
                if !accepts_failure {
                    return;
                }

                let history = self.message_history.entry(*chat_id).or_default();
                history.loading = false;
                history.requested_before_message_id = None;
                history.error = Some(error.clone());
                self.last_error = Some(error.clone());
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
                    let is_newer = chat
                        .last_message
                        .as_ref()
                        .map(|current| {
                            (message.timestamp_unix, message.id)
                                >= (current.timestamp_unix, current.id)
                        })
                        .unwrap_or(true);
                    if is_newer {
                        chat.last_message = Some(message.clone());
                    }
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

    pub fn message_history_for_chat(&self, chat_id: i64) -> Option<&MessageHistoryState> {
        self.message_history.get(&chat_id)
    }

    pub fn total_unread_count(&self) -> u64 {
        self.account
            .chats
            .iter()
            .map(|chat| u64::from(chat.unread_count))
            .sum()
    }

    fn merge_messages(&mut self, chat_id: i64, messages: &[MessageSummary]) {
        for message in messages {
            if message.chat_id == chat_id {
                self.upsert_message(message.clone());
            }
        }

        if let Some(latest) = self
            .messages_for_chat(chat_id)
            .iter()
            .max_by_key(|message| (message.timestamp_unix, message.id))
            .cloned()
        {
            if let Some(chat) = self.account.chats.iter_mut().find(|chat| chat.id == chat_id) {
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
mod message_ordering_tests {
    use super::*;

    fn message(id: i64, timestamp_unix: i64, outgoing: bool) -> MessageSummary {
        MessageSummary {
            id,
            chat_id: 42,
            sender_id: 7,
            text: format!("message {id}"),
            timestamp_unix,
            outgoing,
            attachments: vec![],
            forwarded_from: None,
            edited: false,
            edit_timestamp_unix: None,
            service_message: false,
            media_caption: None,
            reply_to: None,
        }
    }

    #[test]
    fn older_live_update_cannot_replace_chat_preview() {
        let mut state = CoreState::default();
        state.reduce(&CoreEvent::MessageAdded(message(20, 200, false)));
        state.reduce(&CoreEvent::MessageAdded(message(10, 100, false)));
        assert_eq!(
            state.chat(42).unwrap().last_message.as_ref().unwrap().id,
            20
        );
        assert_eq!(state.messages_for_chat(42).len(), 2);
    }

    #[test]
    fn duplicate_update_does_not_increment_unread_count() {
        let mut state = CoreState::default();
        let first = message(20, 200, false);
        state.reduce(&CoreEvent::MessageAdded(first.clone()));
        state.reduce(&CoreEvent::MessageAdded(first));
        assert_eq!(state.chat(42).unwrap().unread_count, 1);
    }

    #[test]
    fn equal_timestamp_uses_message_id_as_tiebreaker() {
        let mut state = CoreState::default();
        state.reduce(&CoreEvent::MessageAdded(message(9, 200, true)));
        state.reduce(&CoreEvent::MessageAdded(message(8, 200, true)));
        assert_eq!(state.chat(42).unwrap().last_message.as_ref().unwrap().id, 9);
        state.reduce(&CoreEvent::MessageAdded(message(10, 200, true)));
        assert_eq!(
            state.chat(42).unwrap().last_message.as_ref().unwrap().id,
            10
        );
    }
}
