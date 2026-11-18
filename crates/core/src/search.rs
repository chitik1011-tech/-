use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    error::{CoreError, Result},
    model::MessageSummary,
};

pub const MAX_MESSAGE_SEARCH_LIMIT: usize = 100;
pub const MAX_MESSAGE_SEARCH_QUERY_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMessagesRequest {
    pub request_id: u64,
    /// Chat ID to scope the search to. `0` requests an account-wide Telegram search.
    pub chat_id: i64,
    pub query: String,
    /// Exclusive Telegram message-id cursor. None starts from newest matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_message_id: Option<i64>,
    pub limit: usize,
}

impl SearchMessagesRequest {
    pub fn validate(&self) -> Result<()> {
        if self.request_id == 0 {
            return Err(CoreError::InvalidInput(
                "message search request id must be non-zero".into(),
            ));
        }
        if self.chat_id < 0 {
            return Err(CoreError::InvalidInput(
                "chat id must not be negative".into(),
            ));
        }
        if self.chat_id == 0 && self.before_message_id.is_some() {
            return Err(CoreError::InvalidInput(
                "account-wide message search does not support pagination yet".into(),
            ));
        }
        let query = self.query.trim();
        if query.is_empty() {
            return Err(CoreError::InvalidInput(
                "message search query is empty".into(),
            ));
        }
        if query.len() > MAX_MESSAGE_SEARCH_QUERY_BYTES {
            return Err(CoreError::InvalidInput(format!(
                "message search query exceeds {MAX_MESSAGE_SEARCH_QUERY_BYTES} UTF-8 bytes"
            )));
        }
        if !(1..=MAX_MESSAGE_SEARCH_LIMIT).contains(&self.limit) {
            return Err(CoreError::InvalidInput(format!(
                "message search limit must be in 1..={MAX_MESSAGE_SEARCH_LIMIT}"
            )));
        }
        if self
            .before_message_id
            .is_some_and(|message_id| message_id <= 0)
        {
            return Err(CoreError::InvalidInput(
                "message search cursor must be positive".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSearchPage {
    pub request_id: u64,
    pub chat_id: i64,
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_before_message_id: Option<i64>,
    pub messages: Vec<MessageSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_before_message_id: Option<i64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveMessageSearch {
    pub request_id: u64,
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_message_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChatMessageSearchState {
    pub current_request_id: Option<u64>,
    pub current_query: Option<String>,
    pub active: Option<ActiveMessageSearch>,
    pub results: Vec<MessageSummary>,
    pub next_before_message_id: Option<i64>,
    pub has_more: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MessageSearchState {
    pub chats: BTreeMap<i64, ChatMessageSearchState>,
}

impl MessageSearchState {
    pub fn begin(&mut self, request: &SearchMessagesRequest) {
        let state = self.chats.entry(request.chat_id).or_default();
        let query = request.query.trim();

        if state
            .current_request_id
            .is_some_and(|current| request.request_id < current)
        {
            return;
        }

        let same_search = state.current_request_id == Some(request.request_id)
            && state.current_query.as_deref() == Some(query);

        if !same_search {
            state.results.clear();
            state.next_before_message_id = None;
            state.has_more = false;
        }

        state.current_request_id = Some(request.request_id);
        state.current_query = Some(query.to_owned());
        state.active = Some(ActiveMessageSearch {
            request_id: request.request_id,
            query: query.to_owned(),
            before_message_id: request.before_message_id,
        });
        state.error = None;
    }

    pub fn complete(&mut self, page: &MessageSearchPage) {
        let Some(state) = self.chats.get_mut(&page.chat_id) else {
            return;
        };
        let Some(active) = state.active.as_ref() else {
            return;
        };

        if active.request_id != page.request_id
            || active.query != page.query.trim()
            || active.before_message_id != page.requested_before_message_id
        {
            return;
        }

        state.active = None;
        for message in &page.messages {
            if message.chat_id == 0 || (page.chat_id != 0 && message.chat_id != page.chat_id) {
                continue;
            }
            if let Some(existing) = state
                .results
                .iter_mut()
                .find(|existing| existing.id == message.id && existing.chat_id == message.chat_id)
            {
                *existing = message.clone();
            } else {
                state.results.push(message.clone());
            }
        }

        state.results.sort_by(|a, b| {
            b.timestamp_unix
                .cmp(&a.timestamp_unix)
                .then_with(|| b.chat_id.cmp(&a.chat_id))
                .then_with(|| b.id.cmp(&a.id))
        });

        let next = page
            .next_before_message_id
            .filter(|message_id| *message_id > 0)
            .filter(|message_id| {
                page.requested_before_message_id
                    .is_none_or(|cursor| *message_id < cursor)
            });
        state.has_more = page.has_more && next.is_some();
        state.next_before_message_id = if state.has_more { next } else { None };
        state.error = None;
    }

    pub fn fail(
        &mut self,
        request_id: u64,
        chat_id: i64,
        query: &str,
        before_message_id: Option<i64>,
        error: &str,
    ) {
        let Some(state) = self.chats.get_mut(&chat_id) else {
            return;
        };
        let Some(active) = state.active.as_ref() else {
            return;
        };
        if active.request_id != request_id
            || active.query != query.trim()
            || active.before_message_id != before_message_id
        {
            return;
        }

        state.active = None;
        state.error = Some(error.to_owned());
    }

    pub fn chat(&self, chat_id: i64) -> Option<&ChatMessageSearchState> {
        self.chats.get(&chat_id)
    }
}
