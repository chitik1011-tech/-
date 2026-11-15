use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("core is not initialized")]
    NotInitialized,

    #[error("core command channel is closed")]
    CommandChannelClosed,

    #[error("backend error: {0}")]
    Backend(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("storage error: {0}")]
    Storage(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
