#![forbid(unsafe_code)]

pub mod backend;
pub mod command;
pub mod error;
pub mod event;
pub mod mock;
pub mod model;
pub mod runtime;
pub mod send;
pub mod state;
pub mod storage;
pub mod transfer;
pub mod wire;

pub use backend::{
    backend_update_channel, BackendFuture, BackendUpdateReceiver, BackendUpdateSender,
    TelegramBackend, DEFAULT_BACKEND_UPDATE_CAPACITY,
};
pub use command::{CoreCommand, MAX_CHAT_LOAD_LIMIT, MAX_MESSAGE_LOAD_LIMIT};
pub use error::{CoreError, Result};
pub use event::CoreEvent;
pub use mock::MockTelegramBackend;
pub use model::{
    AccountState, AttachmentDownloadState, AttachmentKind, AttachmentThumbnail, AuthorizationState,
    ChatSummary, ForwardedFrom, MediaAttachment, MessageSummary, ReplyMetadata,
};
pub use runtime::{spawn_core, spawn_core_with_store, CoreHandle, PERSISTENCE_FLUSH_INTERVAL_MS};
pub use send::{ClientMessageId, OutgoingSend, OutgoingSendState};
pub use state::{CoreState, MAX_CACHED_MESSAGES_PER_CHAT};
pub use storage::{JsonFileStateStore, MemoryStateStore, NoopStateStore, StateStore};
pub use transfer::{
    DownloadState, TransferDirection, TransferError, TransferKey, TransferManager,
    TransferSnapshot, TransferState, TransferToken, UploadState,
};
pub use wire::{decode_command, encode_event, encode_state};
