use serde::{Deserialize, Serialize};

use crate::{
    capabilities::ChatCapabilities,
    chat_pagination::ChatPage,
    connection::ConnectionState,
    connection_machine::{ConnectionToken, LinkEvent},
    contacts::Contact,
    forwarding::ForwardMessagesResult,
    model::{
        AccountState, AttachmentDownloadState, AuthorizationState, ChatActivityKind, ChatSummary,
        LoginCodeDeliveryMethod, MessageReaction, MessageSummary, PeerProfile, ReactionRevision,
    },
    peer::PeerResolution,
    poll::PollSummary,
    search::{MessageSearchPage, SearchMessagesRequest},
    send::OutgoingSend,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageMutationAction {
    Edit,
    DeleteForMe,
    DeleteForEveryone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreEvent {
    State(AccountState),
    AuthorizationChanged(AuthorizationState),
    /// Telegram's stable self user ID for the currently authorized session, or `None` when unavailable.
    AccountIdentityUpdated(Option<i64>),
    /// Actual Telegram code delivery metadata; transient and display-only.
    AuthorizationCodeDeliveryChanged(Option<LoginCodeDeliveryMethod>),
    /// Starts a new connection session and creates its generation token.
    ConnectionSessionStarted,
    /// Lifecycle input produced by work started for a specific connection token.
    ConnectionLifecycle {
        token: ConnectionToken,
        event: LinkEvent,
    },
    /// Update produced by asynchronous work. The actor drops it if its token is stale.
    SessionUpdate {
        token: ConnectionToken,
        event: Box<CoreEvent>,
    },
    /// Legacy untagged state event. Runtime reducers ignore it; use `ConnectionLifecycle`.
    ConnectionStateChanged(ConnectionState),
    ChatsLoaded(Vec<ChatSummary>),
    ChatsPageLoading {
        cursor: Option<String>,
    },
    ChatsPageLoaded(ChatPage),
    ChatsPageFailed {
        cursor: Option<String>,
        error: String,
    },
    /// Replaces one chat's authoritative capability snapshot from Telegram.
    ChatCapabilitiesUpdated(ChatCapabilities),
    /// Replaces the contact snapshot at offset zero and appends later pages.
    ContactsLoaded {
        offset: usize,
        total: usize,
        contacts: Vec<Contact>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },
    /// Marks the newest contact load/search so late results cannot replace it.
    ContactsLoadStarted {
        request_id: u64,
        offset: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
    ContactsLoadFailed {
        request_id: Option<u64>,
        offset: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        error: String,
    },
    /// Telegram invalidated its local contact snapshot; the client should reload it.
    ContactsRefreshRequested,
    MessagesLoaded {
        chat_id: i64,
        messages: Vec<MessageSummary>,
    },
    MessagesPageLoading {
        chat_id: i64,
        before_message_id: Option<i64>,
    },
    MessagesPageLoaded {
        chat_id: i64,
        before_message_id: Option<i64>,
        messages: Vec<MessageSummary>,
        next_before_message_id: Option<i64>,
        has_more: bool,
    },
    MessagesPageLoadFailed {
        chat_id: i64,
        before_message_id: Option<i64>,
        error: String,
    },
    /// Authoritative snapshot of pinned messages returned by Telegram.
    PinnedMessagesLoaded {
        chat_id: i64,
        message_ids: Vec<i64>,
        /// State revision captured when this authoritative snapshot was requested.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_revision: Option<u64>,
    },
    /// Adds or removes pinned message IDs after a Telegram update or successful RPC.
    PinnedMessagesChanged {
        chat_id: i64,
        message_ids: Vec<i64>,
        pinned: bool,
        /// Telegram per-peer update sequence, when supplied by MTProto.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pts: Option<i32>,
    },
    /// A pin or unpin request failed for one specific message.
    PinnedMessageFailed {
        chat_id: i64,
        message_id: i64,
        pinned: bool,
        error: String,
    },
    ChatRead {
        chat_id: i64,
        max_message_id: i64,
    },
    /// Monotonic Telegram read boundary for one side of a chat.
    ReadReceiptUpdated {
        chat_id: i64,
        direction: crate::model::ReadDirection,
        max_message_id: i64,
    },
    MessageSearchStarted(SearchMessagesRequest),
    MessageSearchCompleted(MessageSearchPage),
    MessageSearchFailed {
        request_id: u64,
        chat_id: i64,
        query: String,
        before_message_id: Option<i64>,
        error: String,
    },
    MessageAdded(MessageSummary),
    ProfileUpdated(PeerProfile),
    ChatArchived {
        chat_id: i64,
        archived: bool,
    },
    ChatMuted {
        chat_id: i64,
        muted: bool,
    },
    ChatPinned {
        chat_id: i64,
        pinned: bool,
    },
    ChatUnreadChanged {
        chat_id: i64,
        unread: bool,
    },
    MessageEdited(MessageSummary),
    /// A user requested message mutation failed at the backend; this is an ephemeral UI error.
    MessageMutationSucceeded {
        chat_id: i64,
        message_id: i64,
        action: MessageMutationAction,
    },
    MessageMutationFailed {
        chat_id: i64,
        message_id: i64,
        action: MessageMutationAction,
        error: String,
    },
    MediaTransferUpdated {
        chat_id: i64,
        message_id: i64,
        attachment_index: usize,
        /// Account session owning this download. Stable across reconnects.
        #[serde(default)]
        session_generation: u64,
        #[serde(default)]
        generation: u64,
        state: AttachmentDownloadState,
    },
    MessagesDeleted {
        chat_id: Option<i64>,
        message_ids: Vec<i64>,
    },
    /// Poll snapshot for a message. Replaces the stored snapshot with the same poll id.
    PollUpdated(PollSummary),
    /// The Telegram vote RPC completed successfully. Final choice/results still come from PollUpdated.
    PollVoteSubmitted {
        poll_id: i64,
    },
    /// The Telegram vote RPC failed without replacing the last server-confirmed poll snapshot.
    PollVoteFailed {
        poll_id: i64,
        error: String,
    },
    /// Authoritative Telegram reaction state for a cached message.
    MessageReactionsUpdated {
        chat_id: i64,
        message_id: i64,
        reactions: Vec<MessageReaction>,
        revision: ReactionRevision,
    },
    /// Finishes the UI's pending indicator after the reaction RPC completes.
    ReactionOperationSucceeded {
        chat_id: i64,
        message_id: i64,
    },
    /// Reports a reaction RPC failure without losing the last server snapshot.
    ReactionOperationFailed {
        chat_id: i64,
        message_id: i64,
        error: String,
    },
    /// Forward results; successful messages also arrive as `MessageAdded`.
    MessagesForwarded(ForwardMessagesResult),
    MessageSendStateChanged(OutgoingSend),
    /// `generation` must monotonically increase per (chat_id, user_id).
    ChatActivityChanged {
        chat_id: i64,
        user_id: i64,
        generation: u64,
        activity: ChatActivityKind,
        ttl_ms: u64,
    },
    ChatActivityEnded {
        chat_id: i64,
        user_id: i64,
        generation: u64,
    },
    ChatActivityExpired {
        chat_id: i64,
        user_id: i64,
        generation: u64,
    },
    DraftUpdated(crate::model::Draft),
    DraftCleared {
        chat_id: i64,
        expected_updated_at_unix: Option<i64>,
        #[serde(default)]
        expected_draft: Option<crate::model::Draft>,
    },
    PeerResolved(PeerResolution),
    Error(String),
    Stopped,
}

impl CoreEvent {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::State(_) => "state",
            Self::AuthorizationChanged(_) => "authorization_changed",
            Self::AccountIdentityUpdated(_) => "account_identity_updated",
            Self::AuthorizationCodeDeliveryChanged(_) => "authorization_code_delivery_changed",
            Self::ConnectionSessionStarted => "connection_session_started",
            Self::ConnectionLifecycle { .. } => "connection_lifecycle",
            Self::SessionUpdate { .. } => "session_update",
            Self::ConnectionStateChanged(_) => "connection_state_changed",
            Self::ChatsLoaded(_) => "chats_loaded",
            Self::ChatsPageLoading { .. } => "chats_page_loading",
            Self::ChatsPageLoaded(_) => "chats_page_loaded",
            Self::ChatsPageFailed { .. } => "chats_page_failed",
            Self::ChatCapabilitiesUpdated(_) => "chat_capabilities_updated",
            Self::ContactsLoaded { .. } => "contacts_loaded",
            Self::ContactsLoadStarted { .. } => "contacts_load_started",
            Self::ContactsLoadFailed { .. } => "contacts_load_failed",
            Self::ContactsRefreshRequested => "contacts_refresh_requested",
            Self::MessagesLoaded { .. } => "messages_loaded",
            Self::MessagesPageLoading { .. } => "messages_page_loading",
            Self::MessagesPageLoaded { .. } => "messages_page_loaded",
            Self::MessagesPageLoadFailed { .. } => "messages_page_load_failed",
            Self::PinnedMessagesLoaded { .. } => "pinned_messages_loaded",
            Self::PinnedMessagesChanged { .. } => "pinned_messages_changed",
            Self::PinnedMessageFailed { .. } => "pinned_message_failed",
            Self::ChatRead { .. } => "chat_read",
            Self::ReadReceiptUpdated { .. } => "read_receipt_updated",
            Self::MessageSearchStarted(_) => "message_search_started",
            Self::MessageSearchCompleted(_) => "message_search_completed",
            Self::MessageSearchFailed { .. } => "message_search_failed",
            Self::MessageAdded(_) => "message_added",
            Self::ProfileUpdated(_) => "profile_updated",
            Self::ChatArchived { .. } => "chat_archived",
            Self::ChatMuted { .. } => "chat_muted",
            Self::ChatPinned { .. } => "chat_pinned",
            Self::ChatUnreadChanged { .. } => "chat_unread_changed",
            Self::MessageEdited(_) => "message_edited",
            Self::MessageMutationSucceeded { .. } => "message_mutation_succeeded",
            Self::MessageMutationFailed { .. } => "message_mutation_failed",
            Self::MediaTransferUpdated { .. } => "media_transfer_updated",
            Self::MessagesDeleted { .. } => "messages_deleted",
            Self::PollUpdated(_) => "poll_updated",
            Self::PollVoteSubmitted { .. } => "poll_vote_submitted",
            Self::PollVoteFailed { .. } => "poll_vote_failed",
            Self::MessageReactionsUpdated { .. } => "message_reactions_updated",
            Self::ReactionOperationSucceeded { .. } => "reaction_operation_succeeded",
            Self::ReactionOperationFailed { .. } => "reaction_operation_failed",
            Self::MessagesForwarded(_) => "messages_forwarded",
            Self::MessageSendStateChanged(_) => "message_send_state_changed",
            Self::ChatActivityChanged { .. } => "chat_activity_changed",
            Self::ChatActivityEnded { .. } => "chat_activity_ended",
            Self::ChatActivityExpired { .. } => "chat_activity_expired",
            Self::DraftUpdated(_) => "draft_updated",
            Self::DraftCleared { .. } => "draft_cleared",
            Self::PeerResolved(_) => "peer_resolved",
            Self::Error(_) => "error",
            Self::Stopped => "stopped",
        }
    }

    pub const fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::AccountIdentityUpdated(_)
                | Self::AuthorizationCodeDeliveryChanged(_)
                | Self::ChatCapabilitiesUpdated(_)
                | Self::PollVoteSubmitted { .. }
                | Self::PollVoteFailed { .. }
                | Self::ContactsRefreshRequested
                | Self::ContactsLoadStarted { .. }
                | Self::MessagesForwarded(_)
                | Self::ChatActivityChanged { .. }
                | Self::ChatActivityEnded { .. }
                | Self::ChatActivityExpired { .. }
                | Self::ReactionOperationSucceeded { .. }
                | Self::ReactionOperationFailed { .. }
        )
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::CoreEvent;

    #[test]
    fn account_identity_is_a_transient_stable_kinded_event() {
        let event = CoreEvent::AccountIdentityUpdated(Some(123));

        assert_eq!(event.kind(), "account_identity_updated");
        assert!(event.is_transient());
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({"type":"account_identity_updated","payload":123}),
        );
    }
}
