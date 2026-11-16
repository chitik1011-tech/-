#![forbid(unsafe_code)]

pub mod activity;
pub mod backend;
pub mod command;
pub mod connection;
pub mod connection_machine;
pub mod contacts;
pub mod error;
pub mod event;
pub mod forwarding;
pub mod mock;
pub mod model;
pub mod peer;
pub mod poll;
pub mod runtime;
pub mod search;
pub mod selection;
pub mod send;
pub mod service_message;
pub mod state;
pub mod storage;
pub mod transfer;
pub mod wire;

pub use activity::{ChatActivityTracker, DEFAULT_CHAT_ACTIVITY_TTL_MS, MAX_CHAT_ACTIVITY_TTL_MS};
pub use backend::{
    backend_update_channel, BackendFuture, BackendUpdateReceiver, BackendUpdateSender,
    TelegramBackend, DEFAULT_BACKEND_UPDATE_CAPACITY,
};
pub use command::{
    CoreCommand, MAX_CHAT_LOAD_LIMIT, MAX_EDIT_MESSAGE_TEXT_BYTES, MAX_MESSAGE_LOAD_LIMIT,
    MAX_REACTION_BYTES,
};
pub use connection::ConnectionState;
pub use connection_machine::{
    ConnectionEpoch, ConnectionMachine, ConnectionToken, LinkError, LinkEvent, LinkPhase,
    Outcome as ConnectionOutcome, SessionGeneration,
};
pub use contacts::{Contact, ContactBook, MAX_CONTACT_PAGE_LIMIT};
pub use error::{CoreError, Result};
pub use event::{CoreEvent, MessageMutationAction};
pub use forwarding::{
    ForwardMessagesRequest, ForwardMessagesResult, ForwardOutcome, MAX_FORWARD_MESSAGES,
};
pub use mock::MockTelegramBackend;
pub use model::{
    AccountState, AttachmentDownloadState, AttachmentKind, AttachmentThumbnail, AuthorizationState,
    AvatarReference, ChatActivityKind, ChatFolder, ChatFolderFacts, ChatSummary, Draft,
    ForwardedFrom, GeoPoint, MediaAttachment, MessageHistoryState, MessageLocation,
    MessageReaction, MessageReactionType, MessageSummary, PeerKind, PeerPresence, PeerProfile,
    ReactionRevision, ReadDirection, ReadReceipt, ReplyMetadata, Venue,
};
pub use peer::{
    parse_peer_reference, MessageLink, MessageLinkMetadata, MessageLinkResolution, MessagePeer,
    PeerParseError, PeerReference, PeerResolution, MAX_PEER_REFERENCE_LENGTH,
};
pub use poll::{PollOption, PollSummary, MAX_POLL_OPTIONS};
pub use runtime::{spawn_core, spawn_core_with_store, CoreHandle, PERSISTENCE_FLUSH_INTERVAL_MS};
pub use search::{
    ActiveMessageSearch, ChatMessageSearchState, MessageSearchPage, MessageSearchState,
    SearchMessagesRequest, MAX_MESSAGE_SEARCH_LIMIT, MAX_MESSAGE_SEARCH_QUERY_BYTES,
};
pub use selection::{MessageSelection, SelectionAction, SelectionCapabilities, SelectionError};
pub use send::{ClientMessageId, OutgoingSend, OutgoingSendState};
pub use service_message::{
    ServiceAction, ServiceIcon, ServiceMessage, ServicePresentation, ServiceTone,
};
pub use state::{
    CoreState, FolderAction, MAX_CACHED_MESSAGES_PER_CHAT, MAX_CACHED_POLLS, MAX_CACHED_PROFILES,
};
pub use storage::{JsonFileStateStore, MemoryStateStore, NoopStateStore, StateStore};
pub use transfer::{
    DownloadState, TransferDirection, TransferError, TransferKey, TransferManager,
    TransferSnapshot, TransferState, TransferToken, UploadState,
};
pub use wire::{decode_command, encode_event, encode_state};
