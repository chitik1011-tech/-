use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("core is not initialized")]
    NotInitialized,

    #[error("core command channel is closed")]
    CommandChannelClosed,

    #[error("backend error: {0}")]
    Backend(String),

    /// The request may have reached Telegram, but the client did not receive a
    /// result that proves whether it was accepted. Retrying must reuse its
    /// original client message ID.
    #[error("send outcome is unknown: {0}")]
    SendOutcomeUnknown(String),

    /// A known preflight transport failure happened before a message RPC was
    /// issued, so the same request can be retried safely.
    #[error("temporary Telegram transport failure: {0}")]
    TransientTransport(String),

    /// Telegram returned a temporary server-side RPC failure for this send.
    #[error("temporary Telegram send failure: {0}")]
    TransientSend(String),

    /// Telegram explicitly asked the client to wait before retrying.
    #[error("Telegram FLOOD_WAIT: retry after {seconds} seconds")]
    FloodWait { seconds: u32 },

    /// Telegram explicitly rejected the outgoing message.
    #[error("Telegram rejected the outgoing message: {0}")]
    SendRejected(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("storage error: {0}")]
    Storage(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
