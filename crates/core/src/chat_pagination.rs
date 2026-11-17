use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::model::ChatSummary;

pub const MAX_CHAT_PAGE_LIMIT: usize = 100;
pub const MAX_CHAT_CURSOR_BYTES: usize = 512;

/// One page of the authenticated account's Telegram dialogs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPage {
    /// Cursor used to request this page. `None` identifies the first page.
    pub requested_cursor: Option<String>,
    pub chats: Vec<ChatSummary>,
    pub next_cursor: Option<String>,
}

/// Persisted pagination bookkeeping for the account chat list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChatPaginationState {
    #[serde(default)]
    pub loaded: bool,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub consumed_cursors: BTreeSet<String>,
    #[serde(default)]
    pub loading: bool,
    #[serde(default)]
    pub requested_cursor: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl ChatPaginationState {
    pub fn can_load_next(&self) -> bool {
        !self.loading && (!self.loaded || self.next_cursor.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_allows_first_page_and_limits_retries_to_idle_state() {
        let mut state = ChatPaginationState::default();
        assert!(state.can_load_next());
        state.loading = true;
        assert!(!state.can_load_next());
        state.loading = false;
        state.loaded = true;
        assert!(!state.can_load_next());
        state.next_cursor = Some("100".into());
        assert!(state.can_load_next());
    }
}
