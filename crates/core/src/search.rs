use serde::{Deserialize, Serialize};

use crate::{
    error::{CoreError, Result},
    model::MessageSummary,
};

pub const MAX_MESSAGE_SEARCH_LIMIT: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "scope", content = "chat_id", rename_all = "snake_case")]
pub enum SearchScope {
    Chat(i64),
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMessagesRequest {
    pub request_id: u64,
    pub scope: SearchScope,
    pub query: String,
    pub limit: usize,
    /// Opaque backend cursor. None starts a search at the first page.
    pub cursor: Option<String>,
}

impl SearchMessagesRequest {
    pub fn validate(&self) -> Result<()> {
        if self.query.trim().is_empty() {
            return Err(CoreError::InvalidInput("search query is empty".into()));
        }
        if !(1..=MAX_MESSAGE_SEARCH_LIMIT).contains(&self.limit) {
            return Err(CoreError::InvalidInput(format!(
                "message search limit must be in 1..={MAX_MESSAGE_SEARCH_LIMIT}"
            )));
        }
        if matches!(self.scope, SearchScope::Chat(0)) {
            return Err(CoreError::InvalidInput("chat id must be non-zero".into()));
        }
        if self
            .cursor
            .as_deref()
            .map(str::trim)
            .map_or(false, str::is_empty)
        {
            return Err(CoreError::InvalidInput("search cursor is empty".into()));
        }
        Ok(())
    }
}

/// Metadata returned by the backend with a search hit. These values are never inferred by core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchChatMetadata {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchSenderMetadata {
    pub id: i64,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    pub message: MessageSummary,
    pub chat: SearchChatMetadata,
    pub sender: SearchSenderMetadata,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessagesSearchResult {
    pub request_id: u64,
    pub scope: SearchScope,
    pub query: String,
    /// Echoed by the backend so stale or out-of-order pages can be rejected.
    pub requested_cursor: Option<String>,
    pub results: Vec<SearchResult>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequestState {
    pub request_id: u64,
    pub query: String,
    pub scope: SearchScope,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SearchResultsState {
    pub current_request_id: Option<u64>,
    pub current_query: Option<String>,
    pub active_request: Option<SearchRequestState>,
    pub results: Vec<SearchResult>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MessageSearchState {
    pub global: SearchResultsState,
    pub chats: std::collections::BTreeMap<i64, SearchResultsState>,
}

impl MessageSearchState {
    pub fn begin(&mut self, request: &SearchMessagesRequest) {
        let state = self.results_mut(&request.scope);
        if state
            .current_request_id
            .map_or(false, |current| request.request_id < current)
        {
            return;
        }
        if state.current_request_id == Some(request.request_id)
            && state.current_query.as_deref() != Some(request.query.trim())
        {
            return;
        }
        let same_search = state.current_request_id == Some(request.request_id)
            && state.current_query.as_deref() == Some(request.query.trim());
        if !same_search {
            state.results.clear();
            state.next_cursor = None;
            state.has_more = false;
        }
        state.current_request_id = Some(request.request_id);
        state.current_query = Some(request.query.trim().to_owned());
        state.active_request = Some(SearchRequestState {
            request_id: request.request_id,
            query: request.query.trim().to_owned(),
            scope: request.scope.clone(),
            cursor: request.cursor.clone(),
        });
    }

    pub fn complete(&mut self, page: &MessagesSearchResult) {
        let state = self.results_mut(&page.scope);
        let Some(active) = state.active_request.as_ref() else {
            return;
        };
        if active.request_id != page.request_id
            || active.query != page.query.trim()
            || active.cursor != page.requested_cursor
        {
            return;
        }

        state.active_request = None;
        for result in &page.results {
            if let Some(existing) = state.results.iter_mut().find(|item| {
                item.message.chat_id == result.message.chat_id
                    && item.message.id == result.message.id
            }) {
                *existing = result.clone();
            } else {
                state.results.push(result.clone());
            }
        }
        state.results.sort_by(|a, b| {
            b.message
                .timestamp_unix
                .cmp(&a.message.timestamp_unix)
                .then_with(|| a.message.chat_id.cmp(&b.message.chat_id))
                .then_with(|| a.message.id.cmp(&b.message.id))
        });
        state.next_cursor = page.next_cursor.clone();
        state.has_more = page.has_more && page.next_cursor.is_some();
    }

    fn results_mut(&mut self, scope: &SearchScope) -> &mut SearchResultsState {
        match scope {
            SearchScope::Global => &mut self.global,
            SearchScope::Chat(chat_id) => self.chats.entry(*chat_id).or_default(),
        }
    }
}
