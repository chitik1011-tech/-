use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("core is not initialized")]
    NotInitialized,

    #[error("core command channel is closed")]
    CommandChannelClosed,

    #[error("backend error: {0}")]
    Backend(String),

    #[error("network or transport error: {0}")]
    Transport(String),

    #[error("Telegram flood wait ({seconds:?} seconds)")]
    FloodWait { seconds: Option<u32> },

    #[error("Telegram RPC error {code}: {name}")]
    Rpc { code: i32, name: String },

    #[error("send outcome is unknown: {0}")]
    UnknownSendOutcome(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("storage error: {0}")]
    Storage(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
