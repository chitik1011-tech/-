use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{
    capabilities::ChatCapabilities,
    chat_pagination::ChatPaginationState,
    connection::ConnectionState,
    connection_machine::{ConnectionMachine, LinkEvent, LinkPhase, Outcome},
    contacts::ContactBook,
    event::CoreEvent,
    model::{
        AccountState, AttachmentDownloadState, ChatFolder, ChatSummary, Draft, MessageHistoryState,
        MessageSummary, PeerProfile, ReadDirection, ReadReceipt,
    },
    poll::PollSummary,
    search::MessageSearchState,
    send::{OutgoingSend, OutgoingSendState},
};

pub const MAX_CACHED_MESSAGES_PER_CHAT: usize = 512;
pub const MAX_CACHED_POLLS: usize = 512;
pub const MAX_CACHED_PROFILES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContactsRequestState {
    request_id: u64,
    offset: usize,
    query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreState {
    pub running: bool,
    /// Runtime-only signal: persisted state has finished loading for this handle.
    #[serde(skip)]
    pub restore_complete: bool,
    pub account: AccountState,
    /// Server-provided contact names and stable IDs; phone numbers are never stored here.
    #[serde(default)]
    pub contacts: ContactBook,
    /// Runtime-only correlation key for the newest contact page/search request.
    #[serde(skip)]
    active_contacts_request: Option<ContactsRequestState>,
    #[serde(skip)]
    last_contacts_request_id: Option<u64>,
    /// Session-scoped permissions reported by Telegram, keyed by stable chat ID.
    #[serde(default)]
    pub chat_capabilities: BTreeMap<i64, ChatCapabilities>,
    /// Current Telegram transport state. Older persisted snapshots default to disconnected.
    #[serde(default)]
    pub connection_state: ConnectionState,
    /// Generation-safe connection lifecycle owned by the core actor.
    #[serde(default)]
    pub connection_machine: ConnectionMachine,
    pub messages: BTreeMap<i64, Vec<MessageSummary>>,
    /// Deleted message IDs that may still appear in an in-flight history page.
    /// Older persisted snapshots omit this field and default to an empty map.
    #[serde(default)]
    pub deleted_message_ids: BTreeMap<i64, BTreeSet<i64>>,
    #[serde(default)]
    pub profiles: BTreeMap<i64, PeerProfile>,
    /// FIFO order for the bounded, in-memory profile cache.
    #[serde(default)]
    pub profile_order: VecDeque<i64>,
    /// Telegram's monotonic per-chat inbox and outbox read boundaries.
    #[serde(default)]
    pub read_receipts: BTreeMap<i64, ReadReceipt>,
    #[serde(default)]
    pub message_history: BTreeMap<i64, MessageHistoryState>,
    /// Cursor and retry state for the account's paginated dialog list.
    #[serde(default)]
    pub chat_pagination: ChatPaginationState,
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
    /// Message IDs pinned in each chat, persisted so a reconnect does not erase the UI state.
    #[serde(default)]
    pub pinned_messages: BTreeMap<i64, Vec<i64>>,
    /// Last Telegram update sequence applied for each chat's pinned messages.
    #[serde(default)]
    pub pinned_message_pts: BTreeMap<i64, i32>,
    /// Per-chat revision used to reject snapshots raced by a newer update.
    #[serde(default)]
    pub pinned_message_revision: BTreeMap<i64, u64>,
    /// Per-chat drafts survive navigation and restart.
    #[serde(default)]
    pub drafts: BTreeMap<i64, Draft>,
    pub last_error: Option<String>,
    #[serde(default)]
    pub folders: Vec<ChatFolder>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            running: true,
            restore_complete: false,
            account: AccountState::default(),
            contacts: ContactBook::default(),
            active_contacts_request: None,
            last_contacts_request_id: None,
            chat_capabilities: BTreeMap::new(),
            connection_state: ConnectionState::Disconnected,
            connection_machine: ConnectionMachine::default(),
            messages: BTreeMap::new(),
            deleted_message_ids: BTreeMap::new(),
            profiles: BTreeMap::new(),
            profile_order: VecDeque::new(),
            read_receipts: BTreeMap::new(),
            message_history: BTreeMap::new(),
            chat_pagination: ChatPaginationState::default(),
            message_search: MessageSearchState::default(),
            outbox: BTreeMap::new(),
            send_ledger: BTreeMap::new(),
            polls: BTreeMap::new(),
            poll_order: VecDeque::new(),
            pinned_messages: BTreeMap::new(),
            pinned_message_pts: BTreeMap::new(),
            pinned_message_revision: BTreeMap::new(),
            drafts: BTreeMap::new(),
            last_error: None,
            folders: Vec::new(),
        }
    }
}

impl CoreState {
    /// Clone live state for persistence, omitting session-only contact and
    /// capability data and transient personal profile data.
    pub fn persistence_snapshot(&self) -> Self {
        let mut snapshot = self.clone();
        snapshot.contacts = ContactBook::default();
        snapshot.active_contacts_request = None;
        snapshot.last_contacts_request_id = None;
        snapshot.profiles.clear();
        snapshot.profile_order.clear();
        snapshot.chat_capabilities.clear();
        snapshot.restore_complete = false;
        snapshot
    }

    pub fn profile(&self, peer_id: i64) -> Option<&PeerProfile> {
        self.profiles.get(&peer_id)
    }

    fn clear_authenticated_session(&mut self) {
        let connection_machine = self.connection_machine.clone();
        let running = self.running;
        let restore_complete = self.restore_complete;
        *self = Self::default();
        let connection_state = connection_machine.transport_state();
        self.connection_machine = connection_machine;
        self.connection_state = connection_state;
        self.running = running;
        self.restore_complete = restore_complete;
    }
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
        for (chat_id, message_ids) in &self.deleted_message_ids {
            if *chat_id == 0 || message_ids.iter().any(|id| *id <= 0) {
                return Err(CoreError::InvalidInput(
                    "deleted message state contains an invalid identifier".into(),
                ));
            }
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
        for (chat_id, draft) in &self.drafts {
            if *chat_id != draft.chat_id {
                return Err(CoreError::InvalidInput(
                    "draft map key must match draft chat id".into(),
                ));
            }
            draft.validate()?;
        }
        for (chat_id, message_ids) in &self.pinned_messages {
            if *chat_id == 0 || message_ids.iter().any(|id| *id <= 0) {
                return Err(CoreError::InvalidInput(
                    "pinned message state contains an invalid identifier".into(),
                ));
            }
            let unique = message_ids.iter().copied().collect::<BTreeSet<_>>();
            if unique.len() != message_ids.len() {
                return Err(CoreError::InvalidInput(
                    "pinned message state contains duplicate message ids".into(),
                ));
            }
        }
        for (chat_id, capabilities) in &self.chat_capabilities {
            if *chat_id != capabilities.chat_id {
                return Err(CoreError::InvalidInput(
                    "chat capability map key must match capability chat id".into(),
                ));
            }
            capabilities
                .validate()
                .map_err(|message| CoreError::InvalidInput(message.into()))?;
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
            CoreEvent::ProfileUpdated(profile) => {
                let peer_id = profile.peer_id;
                if !self.profiles.contains_key(&peer_id) {
                    self.profile_order.push_back(peer_id);
                }
                self.profiles.insert(profile.peer_id, profile.clone());
                while self.profile_order.len() > MAX_CACHED_PROFILES {
                    if let Some(evicted) = self.profile_order.pop_front() {
                        self.profiles.remove(&evicted);
                    }
                }
                self.last_error = None;
            }
            CoreEvent::State(account) => {
                self.account = account.clone();
                self.last_error = None;
            }
            CoreEvent::AuthorizationChanged(authorization) => {
                self.account.authorization = authorization.clone();
                self.last_error = None;
            }
            CoreEvent::AccountIdentityUpdated(_) => {
                // Identity is an ephemeral bridge event, not persisted account state.
            }
            CoreEvent::AuthorizationCodeDeliveryChanged(_) => {
                // Login delivery metadata is transient UI state, never persisted.
            }
            CoreEvent::ConnectionSessionStarted => {
                self.connection_machine.start_session();
                self.connection_state = self.connection_machine.transport_state();
                self.chat_capabilities.clear();
            }
            CoreEvent::ConnectionLifecycle { token, event } => {
                let outcome = self.connection_machine.apply(*token, *event);
                if matches!(outcome, Ok(Outcome::Changed { .. } | Outcome::Duplicate)) {
                    self.connection_state = self.connection_machine.transport_state();
                    if matches!(*event, LinkEvent::Logout) {
                        self.clear_authenticated_session();
                    } else if matches!(*event, LinkEvent::TransportLost | LinkEvent::Shutdown) {
                        self.chat_capabilities.clear();
                    }
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
                let known_counters = self
                    .account
                    .chats
                    .iter()
                    .map(|chat| {
                        (
                            chat.id,
                            (chat.unread_mentions_count, chat.unread_reactions_count),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                self.account.chats = chats
                    .iter()
                    .cloned()
                    .map(|mut incoming| {
                        if let Some((mentions, reactions)) = known_counters.get(&incoming.id) {
                            if incoming.unread_mentions_count.is_none() {
                                incoming.unread_mentions_count = *mentions;
                            }
                            if incoming.unread_reactions_count.is_none() {
                                incoming.unread_reactions_count = *reactions;
                            }
                        }
                        incoming
                    })
                    .collect();
                let chat_ids = chats.iter().map(|chat| chat.id).collect::<BTreeSet<_>>();
                self.chat_capabilities
                    .retain(|chat_id, _| chat_ids.contains(chat_id));
                // Bound pin snapshots and per-chat ordering metadata to the recent dialog cache.
                self.pinned_messages
                    .retain(|chat_id, _| chat_ids.contains(chat_id));
                self.pinned_message_pts
                    .retain(|chat_id, _| chat_ids.contains(chat_id));
                self.pinned_message_revision
                    .retain(|chat_id, _| chat_ids.contains(chat_id));
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
            CoreEvent::ChatsPageLoading { cursor } => {
                let pagination = &mut self.chat_pagination;
                pagination.loading = true;
                pagination.requested_cursor = cursor.clone();
                pagination.error = None;
            }
            CoreEvent::ChatsPageLoaded(page) => {
                let pagination = &self.chat_pagination;
                if !pagination.loading || pagination.requested_cursor != page.requested_cursor {
                    return;
                }

                let first_page = page.requested_cursor.is_none();
                let old_chats = std::mem::take(&mut self.account.chats);
                let mut merged = if first_page {
                    Vec::new()
                } else {
                    old_chats.clone()
                };
                for mut incoming in page.chats.iter().cloned() {
                    if let Some(existing) = old_chats.iter().find(|chat| chat.id == incoming.id) {
                        if incoming.unread_mentions_count.is_none() {
                            incoming.unread_mentions_count = existing.unread_mentions_count;
                        }
                        if incoming.unread_reactions_count.is_none() {
                            incoming.unread_reactions_count = existing.unread_reactions_count;
                        }
                        if existing.last_message.as_ref().is_some_and(|old| {
                            incoming
                                .last_message
                                .as_ref()
                                .is_none_or(|new| old.timestamp_unix >= new.timestamp_unix)
                        }) {
                            incoming.last_message = existing.last_message.clone();
                        }
                        if let Some(existing_slot) = merged.iter_mut().find(|c| c.id == incoming.id)
                        {
                            *existing_slot = incoming;
                            continue;
                        }
                    }
                    if !merged.iter().any(|chat| chat.id == incoming.id) {
                        merged.push(incoming);
                    }
                }
                self.account.chats = merged;

                let pagination = &mut self.chat_pagination;
                if first_page {
                    pagination.consumed_cursors.clear();
                }
                if let Some(cursor) = page.requested_cursor.as_ref() {
                    pagination.consumed_cursors.insert(cursor.clone());
                }
                let next_cursor = page.next_cursor.as_ref().filter(|next| {
                    Some(*next) != page.requested_cursor.as_ref()
                        && !pagination.consumed_cursors.contains(*next)
                        && !next.trim().is_empty()
                });
                pagination.loading = false;
                pagination.requested_cursor = None;
                pagination.loaded = true;
                pagination.next_cursor = next_cursor.cloned();
                pagination.error = None;
                if page.next_cursor.is_some() && pagination.next_cursor.is_none() {
                    let error = "Telegram returned a repeated chat page cursor".to_owned();
                    pagination.error = Some(error.clone());
                    self.last_error = Some(error);
                } else {
                    self.last_error = None;
                }
                let ids = self
                    .account
                    .chats
                    .iter()
                    .map(|chat| chat.id)
                    .collect::<BTreeSet<_>>();
                self.chat_capabilities.retain(|id, _| ids.contains(id));
                if first_page && self.connection_machine.phase() == LinkPhase::Synchronizing {
                    let token = self.connection_machine.token();
                    if matches!(
                        self.connection_machine
                            .apply(token, LinkEvent::SyncCompleted),
                        Ok(Outcome::Changed { .. })
                    ) {
                        self.connection_state = self.connection_machine.transport_state();
                    }
                }
            }
            CoreEvent::ChatsPageFailed { cursor, error } => {
                let pagination = &mut self.chat_pagination;
                if !pagination.loading || pagination.requested_cursor != *cursor {
                    return;
                }
                pagination.loading = false;
                pagination.requested_cursor = None;
                if let Some(cursor) = cursor {
                    pagination.next_cursor = Some(cursor.clone());
                }
                pagination.error = Some(error.clone());
                self.last_error = Some(error.clone());
            }
            CoreEvent::ChatCapabilitiesUpdated(capabilities) => {
                if capabilities.validate().is_ok() {
                    self.chat_capabilities
                        .insert(capabilities.chat_id, capabilities.clone());
                }
            }
            CoreEvent::ContactsLoadStarted {
                request_id,
                offset,
                query,
            } => {
                if self
                    .last_contacts_request_id
                    .is_some_and(|last| *request_id <= last)
                {
                    return;
                }
                self.last_contacts_request_id = Some(*request_id);
                self.active_contacts_request = Some(ContactsRequestState {
                    request_id: *request_id,
                    offset: *offset,
                    query: query.clone(),
                });
            }
            CoreEvent::ContactsLoaded {
                offset,
                contacts,
                query,
                request_id,
                ..
            } => {
                if let Some(active) = &self.active_contacts_request {
                    if Some(active.request_id) != *request_id
                        || active.offset != *offset
                        || active.query.as_deref().map(str::trim) != query.as_deref().map(str::trim)
                    {
                        return;
                    }
                } else if request_id.is_some() {
                    return;
                }
                self.active_contacts_request = None;
                self.contacts.apply_page(*offset, contacts);
                self.last_error = None;
            }
            CoreEvent::ContactsLoadFailed {
                request_id,
                offset,
                query,
                error,
            } => {
                if let Some(active) = &self.active_contacts_request {
                    if Some(active.request_id) != *request_id
                        || active.offset != *offset
                        || active.query.as_deref().map(str::trim) != query.as_deref().map(str::trim)
                    {
                        return;
                    }
                } else if request_id.is_some() {
                    return;
                }
                self.active_contacts_request = None;
                self.last_error = Some(error.clone());
            }
            CoreEvent::ContactsRefreshRequested => {}
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

                let next_cursor = (*next_before_message_id)
                    .filter(|message_id| *message_id > 0)
                    .filter(|message_id| {
                        before_message_id.is_none_or(|cursor| *message_id < cursor)
                    });
                let page_has_more = *has_more && next_cursor.is_some();
                self.merge_message_page(*chat_id, messages);

                let history = self.message_history.entry(*chat_id).or_default();
                history.loading = false;
                history.requested_before_message_id = None;
                history.has_more = page_has_more;
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
                self.deleted_message_ids.remove(chat_id);
                self.last_error = Some(error.clone());
            }
            CoreEvent::PinnedMessagesLoaded {
                chat_id,
                message_ids,
                request_revision,
            } => {
                let current_revision = self
                    .pinned_message_revision
                    .get(chat_id)
                    .copied()
                    .unwrap_or_default();
                if *request_revision != Some(current_revision) {
                    return;
                }
                let mut ids = message_ids
                    .iter()
                    .copied()
                    .filter(|id| *id > 0)
                    .collect::<BTreeSet<_>>();
                self.pinned_messages
                    .insert(*chat_id, std::mem::take(&mut ids).into_iter().collect());
                self.pinned_message_revision
                    .insert(*chat_id, current_revision.saturating_add(1));
                self.last_error = None;
            }
            CoreEvent::PinnedMessagesChanged {
                chat_id,
                message_ids,
                pinned,
                pts,
            } => {
                // A no-pts RPC acknowledgement cannot supersede a newer Telegram
                // update. Without a Telegram pts watermark, accept it provisionally.
                let is_newer = match pts {
                    Some(incoming) => self
                        .pinned_message_pts
                        .get(chat_id)
                        .is_none_or(|current| incoming > current),
                    None => !self.pinned_message_pts.contains_key(chat_id),
                };
                if *chat_id != 0 && is_newer {
                    if let Some(pts) = pts {
                        self.pinned_message_pts.insert(*chat_id, *pts);
                    }
                    let ids = self.pinned_messages.entry(*chat_id).or_default();
                    for message_id in message_ids.iter().copied().filter(|id| *id > 0) {
                        if *pinned {
                            if !ids.contains(&message_id) {
                                ids.push(message_id);
                            }
                        } else {
                            ids.retain(|id| *id != message_id);
                        }
                    }
                    ids.sort_unstable();
                    if ids.is_empty() {
                        self.pinned_messages.remove(chat_id);
                    }
                    let revision = self
                        .pinned_message_revision
                        .get(chat_id)
                        .copied()
                        .unwrap_or_default();
                    self.pinned_message_revision
                        .insert(*chat_id, revision.saturating_add(1));
                    self.last_error = None;
                }
            }
            CoreEvent::PinnedMessageFailed { error, .. } => {
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
            CoreEvent::ReadReceiptUpdated {
                chat_id,
                direction,
                max_message_id,
                unread_count,
            } => {
                if *chat_id == 0 || *max_message_id <= 0 {
                    return;
                }
                let previous_inbox_boundary = self
                    .read_receipts
                    .get(chat_id)
                    .map_or(0, |receipt| receipt.inbox_max_message_id);
                let is_fresh_inbox_receipt = *max_message_id >= previous_inbox_boundary;
                if *direction == ReadDirection::Inbox && is_fresh_inbox_receipt {
                    if let Some(chat) = self
                        .account
                        .chats
                        .iter_mut()
                        .find(|chat| chat.id == *chat_id)
                    {
                        if let Some(server_unread_count) = unread_count {
                            // Telegram's still_unread_count is authoritative even when
                            // max_id does not cover the dialog's latest message.
                            chat.unread_count = *server_unread_count;
                        } else {
                            // Backward-compatible events without the server count retain
                            // the previous conservative clear-only behavior.
                            let covers_latest = chat
                                .last_message
                                .as_ref()
                                .is_none_or(|message| message.id <= *max_message_id);
                            if covers_latest {
                                chat.unread_count = 0;
                            }
                        }
                    }
                }
                let receipt = self.read_receipts.entry(*chat_id).or_default();
                let boundary = match direction {
                    ReadDirection::Inbox => &mut receipt.inbox_max_message_id,
                    ReadDirection::Outbox => &mut receipt.outbox_max_message_id,
                };
                *boundary = (*boundary).max(*max_message_id);
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
                let mut message = message.clone();
                if let Some(existing) = self
                    .messages
                    .get(&message.chat_id)
                    .and_then(|messages| messages.iter().find(|cached| cached.id == message.id))
                {
                    preserve_reaction_snapshot(existing, &mut message);
                }
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
            CoreEvent::ChatUnreadMarkChanged { chat_id, unread } => {
                self.update_chat_action(*chat_id, |chat| chat.is_unread = Some(*unread));
            }
            CoreEvent::ChatLeft { chat_id } => {
                self.account.chats.retain(|chat| chat.id != *chat_id);
                self.chat_capabilities.remove(chat_id);
                self.messages.remove(chat_id);
                self.profiles.remove(chat_id);
                self.profile_order.retain(|id| id != chat_id);
                self.read_receipts.remove(chat_id);
                self.message_history.remove(chat_id);
                self.message_search.chats.remove(chat_id);
                self.pinned_messages.remove(chat_id);
                self.pinned_message_pts.remove(chat_id);
                self.pinned_message_revision.remove(chat_id);
                self.drafts.remove(chat_id);
                self.polls.retain(|_, poll| poll.chat_id != *chat_id);
                let live_poll_ids = self.polls.keys().copied().collect::<BTreeSet<_>>();
                self.poll_order
                    .retain(|poll_id| live_poll_ids.contains(poll_id));
                self.outbox.retain(|_, send| send.chat_id != *chat_id);
                self.last_error = None;
            }
            CoreEvent::ChatMemberInvited { .. }
            | CoreEvent::MessageMutationSucceeded { .. }
            | CoreEvent::MessageMutationFailed { .. } => {}
            CoreEvent::MessageEdited(message) => {
                if self.apply_message_edit(message) {
                    self.last_error = None;
                }
            }
            CoreEvent::MediaTransferUpdated {
                chat_id,
                message_id,
                attachment_index,
                session_generation,
                generation,
                state,
            } => {
                if *session_generation == self.connection_machine.token().generation().as_u64()
                    && self.update_attachment_download_state(
                        *chat_id,
                        *message_id,
                        *attachment_index,
                        *generation,
                        state,
                    )
                {
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
                            updated |= self.record_in_flight_page_deletions(*chat_id, &deleted);
                            updated |= self.delete_cached_messages(*chat_id, &deleted);
                        }
                        None => {
                            let loading_chat_ids = self
                                .message_history
                                .iter()
                                .filter(|(_, history)| history.loading)
                                .map(|(chat_id, _)| *chat_id)
                                .collect::<Vec<_>>();
                            for chat_id in loading_chat_ids {
                                updated |= self.record_in_flight_page_deletions(chat_id, &deleted);
                            }
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

                    if let Some(chat_id) = chat_id {
                        if let Some(pinned) = self.pinned_messages.get_mut(chat_id) {
                            let old_len = pinned.len();
                            pinned.retain(|id| !deleted.contains(id));
                            if old_len != pinned.len() {
                                updated = true;
                                let revision = self
                                    .pinned_message_revision
                                    .get(chat_id)
                                    .copied()
                                    .unwrap_or_default();
                                self.pinned_message_revision
                                    .insert(*chat_id, revision.saturating_add(1));
                            }
                            if pinned.is_empty() {
                                self.pinned_messages.remove(chat_id);
                            }
                        }
                    } else {
                        let mut changed_chat_ids = Vec::new();
                        for (pinned_chat_id, pinned) in &mut self.pinned_messages {
                            let old_len = pinned.len();
                            pinned.retain(|id| !deleted.contains(id));
                            if old_len != pinned.len() {
                                updated = true;
                                changed_chat_ids.push(*pinned_chat_id);
                            }
                        }
                        for pinned_chat_id in changed_chat_ids {
                            let revision = self
                                .pinned_message_revision
                                .get(&pinned_chat_id)
                                .copied()
                                .unwrap_or_default();
                            self.pinned_message_revision
                                .insert(pinned_chat_id, revision.saturating_add(1));
                        }
                        self.pinned_messages.retain(|_, ids| !ids.is_empty());
                    }

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
            CoreEvent::PollVoteSubmitted { .. } => {}
            CoreEvent::PollVoteFailed { error, .. } => {
                self.last_error = Some(error.clone());
            }
            CoreEvent::MessageReactionsUpdated {
                chat_id,
                message_id,
                reactions,
                revision,
            } => match self.apply_message_reactions(*chat_id, *message_id, reactions, *revision) {
                Ok(true) => self.last_error = None,
                Ok(false) => {}
                Err(error) => self.last_error = Some(error),
            },
            CoreEvent::ReactionOperationSucceeded { .. } => {}
            CoreEvent::ReactionOperationFailed { .. } => {}
            CoreEvent::MessagesForwarded(result) => {
                self.last_error = result.outcomes.iter().find_map(|outcome| match outcome {
                    crate::forwarding::ForwardOutcome::Failed { error, .. } => Some(error.clone()),
                    crate::forwarding::ForwardOutcome::Forwarded { .. } => None,
                });
            }
            CoreEvent::MessageSendStateChanged(update) => {
                let client_id = update.client_id;
                let mut update = update.clone();
                if update.media.is_some() {
                    let token = self.connection_machine.token();
                    // Media uploads run as detached tasks and may complete after the
                    // connection reconnects within the same account session. Their
                    // transfer_generation orders retries, while session_generation
                    // prevents a result from crossing account replacement. Requiring
                    // an exact connection epoch would strand a valid outbox entry in
                    // Uploading when its final callback arrives after reconnect.
                    if update.session_generation != token.generation().as_u64()
                        || update.session_epoch > token.epoch().as_u64()
                    {
                        return;
                    }
                }
                if let Some(existing) = self.send_ledger.get(&client_id) {
                    if !existing.same_payload(&update) {
                        self.last_error = Some(format!(
                            "client message id {client_id} was reused for a different outgoing message"
                        ));
                        return;
                    }
                    if update.transfer_generation < existing.transfer_generation {
                        return;
                    }
                    if matches!(&update.state, OutgoingSendState::Uploading { .. })
                        && update.transfer_generation == existing.transfer_generation
                        && update.bytes_transferred < existing.bytes_transferred
                    {
                        return;
                    }
                }

                self.send_ledger.insert(client_id, update.clone());
                match &update.state {
                    OutgoingSendState::Sent { .. } | OutgoingSendState::Rejected { .. } => {
                        self.outbox.remove(&client_id);
                    }
                    OutgoingSendState::Sending
                    | OutgoingSendState::Uploading { .. }
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
            CoreEvent::DraftUpdated(draft) => {
                if let Err(error) = draft.validate() {
                    self.last_error = Some(error.to_string());
                } else if self.drafts.get(&draft.chat_id) != Some(draft) {
                    self.drafts.insert(draft.chat_id, draft.clone());
                    self.last_error = None;
                }
            }
            CoreEvent::DraftCleared {
                chat_id,
                expected_updated_at_unix,
                expected_draft,
            } => {
                let should_clear = self.drafts.get(chat_id).is_some_and(|draft| {
                    expected_updated_at_unix.is_none() && expected_draft.is_none()
                        || expected_draft.as_ref().is_some_and(|expected| {
                            expected.chat_id == *chat_id
                                && expected == draft
                                && expected_updated_at_unix
                                    .is_none_or(|timestamp| draft.updated_at_unix == timestamp)
                        })
                });
                if should_clear {
                    self.drafts.remove(chat_id);
                }
            }
            CoreEvent::PeerResolved(_) => {}
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
        let deleted_ids = self
            .deleted_message_ids
            .get(&chat_id)
            .cloned()
            .unwrap_or_default();
        let cache = self.messages.entry(chat_id).or_default();

        for message in messages
            .iter()
            .filter(|message| message.chat_id == chat_id && !deleted_ids.contains(&message.id))
        {
            page_ids.insert(message.id);
            if let Some(index) = cache.iter().position(|cached| cached.id == message.id) {
                let merged = merge_message_snapshot(&cache[index], message);
                cache[index] = merged;
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

        // A new page request is issued only after this response is accepted.
        // Future Telegram reads start after any deletion already observed, so
        // these tombstones are needed only to filter this in-flight response.
        self.deleted_message_ids.remove(&chat_id);

        self.refresh_chat_preview(chat_id);
    }

    fn record_in_flight_page_deletions(&mut self, chat_id: i64, deleted: &BTreeSet<i64>) -> bool {
        let Some(history) = self.message_history.get(&chat_id) else {
            return false;
        };
        if !history.loading {
            return false;
        }

        let cursor = history.requested_before_message_id;
        let relevant = deleted
            .iter()
            .copied()
            .filter(|message_id| cursor.is_none_or(|cursor| *message_id < cursor))
            .collect::<Vec<_>>();
        if !relevant.is_empty() {
            let tombstones = self.deleted_message_ids.entry(chat_id).or_default();
            let before = tombstones.len();
            tombstones.extend(relevant);
            tombstones.len() != before
        } else {
            false
        }
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

        let mut incoming = incoming.clone();
        preserve_reaction_snapshot(current, &mut incoming);

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

    fn update_attachment_download_state(
        &mut self,
        chat_id: i64,
        message_id: i64,
        attachment_index: usize,
        generation: u64,
        state: &AttachmentDownloadState,
    ) -> bool {
        let mut updated = false;
        if let Some(message) = self
            .messages
            .get_mut(&chat_id)
            .and_then(|messages| messages.iter_mut().find(|message| message.id == message_id))
        {
            if let Some(attachment) = message.attachments.get_mut(attachment_index) {
                if accept_media_transfer_update(attachment, generation, state) {
                    attachment.download_generation = generation;
                    attachment.download_state = state.clone();
                    updated = true;
                }
            }
        }
        if let Some(chat) = self
            .account
            .chats
            .iter_mut()
            .find(|chat| chat.id == chat_id)
        {
            if let Some(message) = chat
                .last_message
                .as_mut()
                .filter(|message| message.id == message_id)
            {
                if let Some(attachment) = message.attachments.get_mut(attachment_index) {
                    if accept_media_transfer_update(attachment, generation, state) {
                        attachment.download_generation = generation;
                        attachment.download_state = state.clone();
                        updated = true;
                    }
                }
            }
        }
        updated
    }

    fn apply_message_reactions(
        &mut self,
        chat_id: i64,
        message_id: i64,
        reactions: &[crate::model::MessageReaction],
        revision: crate::model::ReactionRevision,
    ) -> std::result::Result<bool, String> {
        let mut seen = BTreeSet::new();
        for item in reactions {
            item.reaction
                .validate()
                .map_err(|error| error.to_string())?;
            if !seen.insert(item.reaction.clone()) {
                return Err("reaction snapshot contains duplicate reaction identities".into());
            }
        }

        let cached = self
            .messages
            .get(&chat_id)
            .and_then(|messages| messages.iter().find(|message| message.id == message_id))
            .map(|message| (message.reaction_revision, message.reactions.clone()));
        let preview = self
            .chat(chat_id)
            .and_then(|chat| chat.last_message.as_ref())
            .filter(|message| message.id == message_id)
            .map(|message| (message.reaction_revision, message.reactions.clone()));
        let current_revision = cached
            .as_ref()
            .map(|item| item.0)
            .into_iter()
            .chain(preview.as_ref().map(|item| item.0))
            .flatten()
            .max();

        if current_revision.is_some_and(|current| revision <= current) {
            return Ok(false);
        }

        let mut updated = false;
        if let Some(messages) = self.messages.get_mut(&chat_id) {
            if let Some(message) = messages.iter_mut().find(|message| message.id == message_id) {
                message.reactions = reactions.to_vec();
                message.reaction_revision = Some(revision);
                updated = true;
            }
        }
        if let Some(chat) = self
            .account
            .chats
            .iter_mut()
            .find(|chat| chat.id == chat_id)
        {
            if let Some(message) = chat
                .last_message
                .as_mut()
                .filter(|message| message.id == message_id)
            {
                message.reactions = reactions.to_vec();
                message.reaction_revision = Some(revision);
                updated = true;
            }
        }
        Ok(updated)
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
            // Live stream events are applied in receipt order. Keep the latest
            // duplicate event authoritative; only history-page snapshots need
            // edit-aware merging because their request may have predated an edit.
            let mut merged = message;
            preserve_reaction_snapshot(&messages[index], &mut merged);
            messages[index] = merged;
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

fn accept_media_transfer_update(
    attachment: &crate::model::MediaAttachment,
    generation: u64,
    incoming: &AttachmentDownloadState,
) -> bool {
    if generation < attachment.download_generation {
        return false;
    }
    if generation > attachment.download_generation {
        return true;
    }

    use AttachmentDownloadState as State;
    let current = &attachment.download_state;
    let rank = |state: &State| match state {
        State::NotDownloaded => 0,
        State::Queued => 1,
        State::Downloading { .. } => 2,
        State::Downloaded { .. } | State::Failed { .. } | State::Cancelled => 3,
    };
    if rank(incoming) < rank(current) {
        return false;
    }
    if rank(current) == 3 && incoming != current {
        return false;
    }
    if let (
        State::Downloading {
            bytes_transferred: old,
            ..
        },
        State::Downloading {
            bytes_transferred: new,
            ..
        },
    ) = (current, incoming)
    {
        if new < old {
            return false;
        }
    }
    current != incoming
}

fn preserve_reaction_snapshot(existing: &MessageSummary, incoming: &mut MessageSummary) {
    let keep_existing = match (existing.reaction_revision, incoming.reaction_revision) {
        (Some(current), Some(candidate)) => candidate <= current,
        (Some(_), None) => true,
        _ => false,
    };
    if keep_existing {
        incoming.reactions.clone_from(&existing.reactions);
        incoming.reaction_revision = existing.reaction_revision;
    }
}

fn merge_message_snapshot(existing: &MessageSummary, incoming: &MessageSummary) -> MessageSummary {
    let mut merged = if message_edit_is_newer(existing, incoming) {
        incoming.clone()
    } else {
        existing.clone()
    };
    preserve_reaction_snapshot(existing, &mut merged);
    merged
}

#[cfg(test)]
mod message_ordering_tests {
    use super::*;

    fn seed_chat(state: &mut CoreState, unread_count: u32) {
        state.account.chats.push(ChatSummary {
            id: 42,
            title: "Known chat".into(),
            unread_count,
            archived: None,
            muted: None,
            pinned: None,
            is_unread: None,
            unread_mentions_count: None,
            unread_reactions_count: None,
            last_message: None,
        });
    }

    fn message(id: i64, timestamp_unix: i64, outgoing: bool) -> MessageSummary {
        MessageSummary {
            id,
            chat_id: 42,
            sender_id: 7,
            text: format!("message {id}"),
            timestamp_unix,
            outgoing,
            location: None,
            attachments: vec![],
            forwarded_from: None,
            edited: false,
            edit_timestamp_unix: None,
            service_message: false,
            service: None,
            media_caption: None,
            reply_to: None,
            reactions: Vec::new(),
            reaction_revision: None,
        }
    }

    #[test]
    fn older_live_update_cannot_replace_chat_preview() {
        let mut state = CoreState::default();
        seed_chat(&mut state, 0);
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
        seed_chat(&mut state, 0);
        let first = message(20, 200, false);
        state.reduce(&CoreEvent::MessageAdded(first.clone()));
        state.reduce(&CoreEvent::MessageAdded(first));
        assert_eq!(state.chat(42).unwrap().unread_count, 1);
    }

    #[test]
    fn equal_timestamp_uses_message_id_as_tiebreaker() {
        let mut state = CoreState::default();
        seed_chat(&mut state, 0);
        state.reduce(&CoreEvent::MessageAdded(message(9, 200, true)));
        state.reduce(&CoreEvent::MessageAdded(message(8, 200, true)));
        assert_eq!(state.chat(42).unwrap().last_message.as_ref().unwrap().id, 9);
        state.reduce(&CoreEvent::MessageAdded(message(10, 200, true)));
        assert_eq!(
            state.chat(42).unwrap().last_message.as_ref().unwrap().id,
            10
        );
    }

    #[test]
    fn logout_invalidates_generation_and_clears_account_scoped_caches() {
        let mut state = CoreState::default();
        seed_chat(&mut state, 0);
        state.reduce(&CoreEvent::MessageAdded(message(7, 1, false)));
        state.pinned_messages.insert(42, vec![7]);
        state
            .outbox
            .insert(1, OutgoingSend::sending(1, 42, "pending"));
        state
            .send_ledger
            .insert(1, OutgoingSend::sending(1, 42, "pending"));
        state.last_error = Some("old session error".into());

        state.reduce(&CoreEvent::ConnectionSessionStarted);
        let token = state.connection_machine.token();
        state.reduce(&CoreEvent::ConnectionLifecycle {
            token,
            event: LinkEvent::TransportConnected,
        });
        let token = state.connection_machine.token();
        state.reduce(&CoreEvent::ConnectionLifecycle {
            token,
            event: LinkEvent::SyncStarted,
        });
        let token = state.connection_machine.token();
        state.reduce(&CoreEvent::ConnectionLifecycle {
            token,
            event: LinkEvent::SyncCompleted,
        });
        let token = state.connection_machine.token();

        state.reduce(&CoreEvent::ConnectionLifecycle {
            token,
            event: LinkEvent::Logout,
        });

        assert!(state.account.chats.is_empty());
        assert!(state.messages.is_empty());
        assert!(state.pinned_messages.is_empty());
        assert!(state.outbox.is_empty());
        assert!(state.send_ledger.is_empty());
        assert!(state.last_error.is_none());
        assert_eq!(state.connection_machine.phase(), LinkPhase::Disconnected);
        assert_ne!(state.connection_machine.token(), token);
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
