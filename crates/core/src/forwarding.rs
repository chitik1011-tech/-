use serde::{Deserialize, Serialize};

use crate::{error::{CoreError, Result}, model::MessageRef};

pub const MAX_FORWARD_MESSAGES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardMessagesRequest {
    /// Source message references in the order Telegram should forward them.
    pub sources: Vec<MessageRef>,
    /// One destination chat for the complete ordered batch.
    pub destination_chat_id: i64,
    /// Stable caller supplied id used to correlate and deduplicate retries.
    pub request_id: String,
}

impl ForwardMessagesRequest {
    pub fn validate(&self) -> Result<()> {
        if self.sources.is_empty() || self.sources.len() > MAX_FORWARD_MESSAGES {
            return Err(CoreError::InvalidInput(format!("forward source count must be in 1..={MAX_FORWARD_MESSAGES}")));
        }
        if self.destination_chat_id == 0 {
            return Err(CoreError::InvalidInput("destination chat id must be non-zero".into()));
        }
        if self.request_id.trim().is_empty() {
            return Err(CoreError::InvalidInput("forward request id is empty".into()));
        }
        let mut seen = std::collections::HashSet::new();
        for source in &self.sources {
            if source.chat_id == 0 || source.message_id <= 0 {
                return Err(CoreError::InvalidInput("source chat id must be non-zero and message id positive".into()));
            }
            if !seen.insert((source.chat_id, source.message_id)) {
                return Err(CoreError::InvalidInput("forward source references must be unique".into()));
            }
        }
        Ok(())
    }
}

/// Per-source outcome. Successful IDs identify the newly forwarded Telegram
/// message; message text is deliberately absent because forwarding is not copying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ForwardOutcome {
    Forwarded { source: MessageRef, destination_message_id: i64 },
    Failed { source: MessageRef, error: String },
}

impl ForwardOutcome {
    pub fn source(&self) -> &MessageRef {
        match self { Self::Forwarded { source, .. } | Self::Failed { source, .. } => source }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardMessagesResult {
    pub request_id: String,
    pub source_chat_id: i64,
    pub destination_chat_id: i64,
    /// One outcome per requested source, in request order; partial failures are explicit.
    pub outcomes: Vec<ForwardOutcome>,
}
