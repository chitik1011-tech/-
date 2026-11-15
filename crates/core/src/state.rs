use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{
    connection::ConnectionState,
    connection_machine::{ConnectionMachine, LinkEvent, LinkPhase, Outcome},
    event::CoreEvent,
    model::{AccountState, ChatFolder, ChatSummary, MessageHistoryState, MessageSummary},
    poll::PollSummary,
    search::MessageSearchState,
    send::{OutgoingSend, OutgoingSendState},
};

pub const MAX_CACHED_MESSAGES_PER_CHAT: usize = 512;
pub const MAX_CACHED_POLLS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreState {
    pub running: bool,
    pub account: AccountState,
    /// Current Telegram transport state. Older persisted snapshots default to disconnected.
    #[serde(default)]
    pub connection_state: ConnectionState,
    /// Generation-safe connection lifecycle owned by the core actor.
    #[serde(default)]
    pub connection_machine: ConnectionMachine,
    pub messages: BTreeMap<i64, Vec<MessageSummary>>,
    #[serde(default)]
    pub message_history: BTreeMap<i64, MessageHistoryState>,
    #[serde(default)]
    pub message_search: MessageSearchState,
    /// Reliable sends that are still in flight or failed and can be retried.
    /// Older persisted state did not contain this field.
    #[serde(default)]
    pub outbox: BTreeMap<i64, OutgoingSend>,
    /// Persistent client-ID binding and final result ledger. Retaining this
    /// ledger prevents a client ID from being reused after acknowledgment or
    /// process restart.
    #[serde(default)]
    pub send_ledger: BTreeMap<i64, OutgoingSend>,
    /// Telegram polls keyed by poll identifier. Older persisted state did not
    /// contain polls and defaults to an empty map.
    #[serde(default)]
    pub polls: BTreeMap<i64, PollSummary>,
    /// Insertion order for deterministic FIFO poll eviction. Poll updates keep
    /// their original position so high-traffic polls cannot evict newer polls.
    #[serde(default)]
    pub poll_order: VecDeque<i64>,
    pub last_error: Option<String>,
    #[serde(default)]
    pub folders: Vec<ChatFolder>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            running: true,
            account: AccountState::default(),
            connection_state: ConnectionState::Disconnected,
            connection_machine: ConnectionMachine::default(),
            messages: BTreeMap::new(),
            message_history: BTreeMap::new(),
            message_search: MessageSearchState::default(),
            outbox: BTreeMap::new(),
            send_ledger: BTreeMap::new(),
            polls: BTreeMap::new(),
            poll_order: VecDeque::new(),
            last_error: None,
            folders: Vec::new(),
        }
    }
}

impl CoreState {
    /// Validate persisted or externally constructed state invariants.
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::CoreError;

        let mut ids = std::collections::BTreeSet::new();
        for (index, folder) in self.folders.iter().enumerate() {
            folder.validate()?;
            if !ids.insert(folder.id) {
                return Err(CoreError::InvalidInput(format!(
                    "duplicate folder id: {}",
                    folder.id
                )));
            }
            if folder.position as usize != index {
                return Err(CoreError::InvalidInput(
                    "folder positions must be contiguous and match list order".into(),
                ));
            }
        }

        if self.polls.len() > MAX_CACHED_POLLS {
            return Err(CoreError::InvalidInput(format!(
                "poll cache exceeds {MAX_CACHED_POLLS} entries"
            )));
        }
        let mut ordered_poll_ids = BTreeSet::new();
        if self
            .poll_order
            .iter()
            .any(|id| !self.polls.contains_key(id) || !ordered_poll_ids.insert(*id))
            || ordered_poll_ids.len() != self.polls.len()
        {
            return Err(CoreError::InvalidInput(
                "poll cache order must contain every poll id exactly once".into(),
            ));
        }
        for (poll_id, poll) in &self.polls {
            if *poll_id != poll.id {
                return Err(CoreError::InvalidInput(
                    "poll cache key must match poll id".into(),
                ));
            }
            poll.validate()?;
        }
        Ok(())
    }

    /// Apply a deterministic saved-folder change and keep positions contiguous.
    pub fn reduce_folder(&mut self, action: FolderAction) -> crate::error::Result<()> {
        use crate::error::CoreError;

        self.validate()?;
        match action {
            FolderAction::Upsert(folder) => {
                folder.validate()?;
                let existing_index = self.folders.iter().position(|item| item.id == folder.id);
                let old_len = self.folders.len();
                if folder.position as usize > old_len {
                    return Err(CoreError::InvalidInput(format!(
                        "folder position must be in 0..={old_len}"
                    )));
                }
                if let Some(index) = existing_index {
                    self.folders.remove(index);
                }
                let insertion_index = (folder.position as usize).min(self.folders.len());
                self.folders.insert(insertion_index, folder);
                self.normalize_folder_positions();
            }
            FolderAction::Remove { folder_id } => {
                if folder_id <= 0 {
                    return Err(CoreError::InvalidInput("folder id must be positive".into()));
                }
                self.folders.retain(|folder| folder.id != folder_id);
                self.normalize_folder_positions();
            }
            FolderAction::Reorder { folder_ids } => {
                if folder_ids.len() != self.folders.len() {
                    return Err(CoreError::InvalidInput(
                        "folder order must list every folder exactly once".into(),
                    ));
                }
                let mut unique_ids = std::collections::BTreeSet::new();
                if folder_ids.iter().any(|id| !unique_ids.insert(*id))
                    || folder_ids
                        .iter()
                        .any(|id| !self.folders.iter().any(|folder| folder.id == *id))
                {
                    return Err(CoreError::InvalidInput(
                        "folder order contains duplicate or unknown ids".into(),
                    ));
                }
                let order: BTreeMap<_, _> = folder_ids
                    .into_iter()
                    .enumerate()
                    .map(|(position, id)| (id, position))
                    .collect();
                self.folders
                    .sort_by_key(|folder| order.get(&folder.id).copied().unwrap_or(usize::MAX));
                self.normalize_folder_positions();
            }
        }
        Ok(())
    }

    fn normalize_folder_positions(&mut self) {
        for (index, folder) in self.folders.iter_mut().enumerate() {
            folder.position = index as u32;
        }
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
            CoreEvent::ConnectionSessionStarted => {
                self.connection_machine.start_session();
                self.connection_state = self.connection_machine.transport_state();
            }
            CoreEvent::ConnectionLifecycle { token, event } => {
                if matches!(
                    self.connection_machine.apply(*token, *event),
                    Ok(Outcome::Changed { .. })
                ) {
                    self.connection_state = self.connection_machine.transport_state();
                }
            }
            CoreEvent::SessionUpdate { token, event } => {
                if *token == self.connection_machine.token() {
                    self.reduce(event);
                }
            }
            CoreEvent::ConnectionStateChanged(_) => {
                // Untagged connection callbacks cannot be made generation-safe.
            }
            CoreEvent::ChatsLoaded(chats) => {
                self.account.chats = chats.clone();
                if self.connection_machine.phase() == LinkPhase::Synchronizing {
                    let token = self.connection_machine.token();
                    if matches!(
                        self.connection_machine
                            .apply(token, LinkEvent::SyncCompleted),
                        Ok(Outcome::Changed { .. })
                    ) {
                        self.connection_state = self.connection_machine.transport_state();
                    }
                }
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
                    history.loading && history.requested_before_message_id == *before_message_id
                });
                if !accepts_page {
                    return;
                }

                self.merge_message_page(*chat_id, messages);

                let next_cursor = (*next_before_message_id)
                    .filter(|message_id| *message_id > 0)
                    .filter(|message_id| {
                        before_message_id.is_none_or(|cursor| *message_id < cursor)
                    });
                let history = self.message_history.entry(*chat_id).or_default();
                history.loading = false;
                history.requested_before_message_id = None;
                history.has_more = *has_more && next_cursor.is_some();
                history.next_before_message_id = if history.has_more { next_cursor } else { None };
                history.error = None;
                self.last_error = None;
            }
            CoreEvent::MessagesPageLoadFailed {
                chat_id,
                before_message_id,
                error,
            } => {
                let accepts_failure = self.message_history.get(chat_id).is_some_and(|history| {
                    history.loading && history.requested_before_message_id == *before_message_id
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
            CoreEvent::MessageSearchStarted(request) => {
                self.message_search.begin(request);
            }
            CoreEvent::MessageSearchCompleted(page) => {
                self.message_search.complete(page);
            }
            CoreEvent::MessageSearchFailed {
                request_id,
                chat_id,
                query,
                before_message_id,
                error,
            } => {
                self.message_search
                    .fail(*request_id, *chat_id, query, *before_message_id, error);
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
                        chat.is_unread = Some(true);
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
                        archived: Some(false),
                        muted: Some(false),
                        pinned: Some(false),
                        is_unread: Some(!message.outgoing),
                        last_message: Some(message.clone()),
                    });
                }

                self.last_error = None;
            }
            CoreEvent::ChatArchived { chat_id, archived } => {
                self.update_chat_action(*chat_id, |chat| chat.archived = Some(*archived));
            }
            CoreEvent::ChatMuted { chat_id, muted } => {
                self.update_chat_action(*chat_id, |chat| chat.muted = Some(*muted));
            }
            CoreEvent::ChatPinned { chat_id, pinned } => {
                self.update_chat_action(*chat_id, |chat| chat.pinned = Some(*pinned));
            }
            CoreEvent::ChatUnreadChanged { chat_id, unread } => {
                self.update_chat_action(*chat_id, |chat| {
                    chat.is_unread = Some(*unread);
                    if !*unread {
                        chat.unread_count = 0;
                    }
                });
            }
            CoreEvent::MessageEdited(message) => {
                if self.apply_message_edit(message) {
                    self.last_error = None;
                }
            }
            CoreEvent::MessagesDeleted {
                chat_id,
                message_ids,
            } => {
                let deleted = message_ids
                    .iter()
                    .copied()
                    .filter(|message_id| *message_id > 0)
                    .collect::<BTreeSet<_>>();

                if !deleted.is_empty() {
                    let mut updated = false;
                    match chat_id {
                        Some(chat_id) => {
                            updated = self.delete_cached_messages(*chat_id, &deleted);
                        }
                        None => {
                            let mut chat_ids =
                                self.messages.keys().copied().collect::<BTreeSet<_>>();
                            chat_ids.extend(self.account.chats.iter().map(|chat| chat.id));
                            for chat_id in chat_ids {
                                updated |= self.delete_cached_messages(chat_id, &deleted);
                            }
                        }
                    }

                    // A poll lives on its message: drop polls whose message was deleted.
                    let polls_before = self.polls.len();
                    self.polls.retain(|_, poll| {
                        let chat_matches = match chat_id {
                            Some(chat) => *chat == poll.chat_id,
                            None => true,
                        };
                        !(chat_matches && deleted.contains(&poll.message_id))
                    });
                    updated |= self.polls.len() != polls_before;
                    let order_before = self.poll_order.len();
                    self.poll_order
                        .retain(|poll_id| self.polls.contains_key(poll_id));
                    updated |= self.poll_order.len() != order_before;

                    if updated {
                        self.last_error = None;
                    }
                }
            }
            CoreEvent::PollUpdated(poll) => {
                if let Err(error) = poll.validate() {
                    self.last_error = Some(error.to_string());
                } else {
                    match self.apply_poll(poll) {
                        Ok(_) => self.last_error = None,
                        Err(error) => self.last_error = Some(error),
                    }
                }
            }
            CoreEvent::MessageSendStateChanged(update) => {
                let client_id = update.client_id;
                if let Some(existing) = self.send_ledger.get(&client_id) {
                    if !existing.same_request(update.chat_id, &update.text) {
                        self.last_error = Some(format!(
                            "client message id {client_id} was reused for a different outgoing message"
                        ));
                        return;
                    }
                }

                self.send_ledger.insert(client_id, update.clone());
                match &update.state {
                    OutgoingSendState::Sent { .. } | OutgoingSendState::Rejected { .. } => {
                        self.outbox.remove(&client_id);
                    }
                    OutgoingSendState::Sending
                    | OutgoingSendState::AwaitingConfirmation { .. }
                    | OutgoingSendState::FloodWait { .. }
                    | OutgoingSendState::Failed {
                        retryable: true, ..
                    } => {
                        self.outbox.insert(client_id, update.clone());
                    }
                    OutgoingSendState::Failed {
                        retryable: false, ..
                    } => {
                        self.outbox.remove(&client_id);
                    }
                }
            }
            CoreEvent::ChatActivityChanged { .. }
            | CoreEvent::ChatActivityEnded { .. }
            | CoreEvent::ChatActivityExpired { .. } => {}
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

    pub fn outgoing_send(&self, client_id: i64) -> Option<&OutgoingSend> {
        self.outbox.get(&client_id)
    }

    /// Poll snapshot by Telegram poll identifier.
    pub fn poll(&self, poll_id: i64) -> Option<&PollSummary> {
        self.polls.get(&poll_id)
    }

    /// Poll attached to a specific message, if one is stored.
    pub fn poll_for_message(&self, chat_id: i64, message_id: i64) -> Option<&PollSummary> {
        self.polls
            .values()
            .find(|poll| poll.chat_id == chat_id && poll.message_id == message_id)
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

        self.refresh_chat_preview(chat_id);
    }

    fn merge_message_page(&mut self, chat_id: i64, messages: &[MessageSummary]) {
        let mut page_ids = BTreeSet::new();
        let cache = self.messages.entry(chat_id).or_default();

        for message in messages.iter().filter(|message| message.chat_id == chat_id) {
            page_ids.insert(message.id);
            if let Some(index) = cache.iter().position(|cached| cached.id == message.id) {
                cache[index] = message.clone();
            } else {
                cache.push(message.clone());
            }
        }

        cache.sort_by_key(|message| (message.timestamp_unix, message.id));

        // When an older page arrives into a full cache, preserve the page the
        // caller just requested. Evict newer cached messages from the tail;
        // those can be loaded again by paging from the latest end.
        let excess = cache.len().saturating_sub(MAX_CACHED_MESSAGES_PER_CHAT);
        if excess > 0 {
            let evicted_ids = cache
                .iter()
                .rev()
                .filter(|message| !page_ids.contains(&message.id))
                .take(excess)
                .map(|message| message.id)
                .collect::<BTreeSet<_>>();
            cache.retain(|message| !evicted_ids.contains(&message.id));
        }

        self.refresh_chat_preview(chat_id);
    }

    fn refresh_chat_preview(&mut self, chat_id: i64) {
        if let Some(latest) = self
            .messages_for_chat(chat_id)
            .iter()
            .max_by_key(|message| (message.timestamp_unix, message.id))
            .cloned()
        {
            if let Some(chat) = self
                .account
                .chats
                .iter_mut()
                .find(|chat| chat.id == chat_id)
            {
                let replace = chat
                    .last_message
                    .as_ref()
                    .map(|current| {
                        (latest.timestamp_unix, latest.id) > (current.timestamp_unix, current.id)
                    })
                    .unwrap_or(true);
                if replace {
                    chat.last_message = Some(latest);
                }
            }
        }
    }

    fn apply_message_edit(&mut self, incoming: &MessageSummary) -> bool {
        let cached = self
            .messages
            .get(&incoming.chat_id)
            .and_then(|messages| messages.iter().find(|item| item.id == incoming.id))
            .cloned();
        let preview = self
            .chat(incoming.chat_id)
            .and_then(|chat| chat.last_message.as_ref())
            .filter(|item| item.id == incoming.id)
            .cloned();

        let Some(current) = cached
            .iter()
            .chain(preview.iter())
            .max_by_key(|item| (item.edit_timestamp_unix, item.edited))
        else {
            return false;
        };
        if !message_edit_is_newer(current, incoming) {
            return false;
        }

        let mut updated = false;
        if let Some(messages) = self.messages.get_mut(&incoming.chat_id) {
            if let Some(existing) = messages.iter_mut().find(|item| item.id == incoming.id) {
                *existing = incoming.clone();
                updated = true;
            }
        }
        if let Some(chat) = self
            .account
            .chats
            .iter_mut()
            .find(|chat| chat.id == incoming.chat_id)
        {
            if chat
                .last_message
                .as_ref()
                .is_some_and(|last| last.id == incoming.id)
            {
                chat.last_message = Some(incoming.clone());
                updated = true;
            }
        }
        updated
    }

    fn delete_cached_messages(&mut self, chat_id: i64, deleted: &BTreeSet<i64>) -> bool {
        let mut updated = false;
        let remove_entry = if let Some(messages) = self.messages.get_mut(&chat_id) {
            let original_len = messages.len();
            messages.retain(|message| !deleted.contains(&message.id));
            updated = messages.len() != original_len;
            messages.is_empty()
        } else {
            false
        };
        if remove_entry {
            self.messages.remove(&chat_id);
        }

        let replacement = self
            .messages_for_chat(chat_id)
            .iter()
            .max_by_key(|message| (message.timestamp_unix, message.id))
            .cloned();

        if let Some(chat) = self
            .account
            .chats
            .iter_mut()
            .find(|chat| chat.id == chat_id)
        {
            if chat
                .last_message
                .as_ref()
                .is_some_and(|message| deleted.contains(&message.id))
            {
                chat.last_message = replacement;
                updated = true;
            }
        }
        updated
    }

    /// Stores a poll snapshot. Returns `Ok(true)` when the stored poll changed,
    /// `Ok(false)` for a duplicate or stale snapshot, and an error when the same
    /// Telegram poll id is reported for a different message.
    ///
    /// Telegram sends no ordering for poll results, so the newest accepted
    /// snapshot wins. A closed poll is final: a later open snapshot is stale.
    fn apply_poll(&mut self, incoming: &PollSummary) -> std::result::Result<bool, String> {
        let mut ordered_poll_ids = BTreeSet::new();
        if self.poll_order.len() != self.polls.len()
            || self
                .poll_order
                .iter()
                .any(|id| !self.polls.contains_key(id) || !ordered_poll_ids.insert(*id))
        {
            return Err("poll cache order is inconsistent with stored polls".into());
        }

        if let Some(existing) = self.polls.get(&incoming.id) {
            if existing.chat_id != incoming.chat_id || existing.message_id != incoming.message_id {
                return Err(format!(
                    "poll {} was reported for a different message",
                    incoming.id
                ));
            }
            if existing.closed && !incoming.closed {
                return Ok(false);
            }
            if existing == incoming {
                return Ok(false);
            }
        } else if self.polls.values().any(|existing| {
            existing.chat_id == incoming.chat_id && existing.message_id == incoming.message_id
        }) {
            return Err(format!(
                "message {} in chat {} already has a different poll",
                incoming.message_id, incoming.chat_id
            ));
        }

        if !self.polls.contains_key(&incoming.id) {
            if self.polls.len() >= MAX_CACHED_POLLS {
                let oldest_poll_id = self
                    .poll_order
                    .pop_front()
                    .ok_or_else(|| "poll cache is full but has no eviction order".to_owned())?;
                self.polls.remove(&oldest_poll_id);
            }
            self.poll_order.push_back(incoming.id);
        }

        self.polls.insert(incoming.id, incoming.clone());
        Ok(true)
    }

    fn update_chat_action(&mut self, chat_id: i64, update: impl FnOnce(&mut ChatSummary)) {
        if let Some(chat) = self
            .account
            .chats
            .iter_mut()
            .find(|chat| chat.id == chat_id)
        {
            update(chat);
            self.last_error = None;
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

fn message_edit_is_newer(current: &MessageSummary, incoming: &MessageSummary) -> bool {
    if current == incoming {
        return false;
    }

    match (current.edit_timestamp_unix, incoming.edit_timestamp_unix) {
        (Some(current_timestamp), Some(incoming_timestamp)) => {
            incoming_timestamp > current_timestamp
                || (incoming_timestamp == current_timestamp && !current.edited && incoming.edited)
        }
        (Some(_), None) => false,
        (None, Some(_)) => true,
        (None, None) => !current.edited && incoming.edited,
    }
}
/// Reducer inputs for saved chat folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderAction {
    Upsert(ChatFolder),
    Remove { folder_id: i64 },
    Reorder { folder_ids: Vec<i64> },
}
