use serde::{Deserialize, Serialize};

/// Stable client-generated identifier for one logical outgoing message.
///
/// The same identifier may be retried with the same chat/text. Reusing it for
/// different content is invalid because Telegram uses it as the MTProto
/// `random_id` idempotency key.
pub type ClientMessageId = i64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingSend {
    pub client_id: ClientMessageId,
    pub chat_id: i64,
    pub text: String,
    pub state: OutgoingSendState,
}

impl OutgoingSend {
    pub fn sending(client_id: ClientMessageId, chat_id: i64, text: impl Into<String>) -> Self {
        Self {
            client_id,
            chat_id,
            text: text.into(),
            state: OutgoingSendState::Sending,
        }
    }

    pub fn sent(
        client_id: ClientMessageId,
        chat_id: i64,
        text: impl Into<String>,
        message_id: i64,
    ) -> Self {
        Self {
            client_id,
            chat_id,
            text: text.into(),
            state: OutgoingSendState::Sent { message_id },
        }
    }

    pub fn failed(
        client_id: ClientMessageId,
        chat_id: i64,
        text: impl Into<String>,
        reason: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            client_id,
            chat_id,
            text: text.into(),
            state: OutgoingSendState::Failed {
                reason: reason.into(),
                retryable,
            },
        }
    }

    pub fn same_request(&self, chat_id: i64, text: &str) -> bool {
        self.chat_id == chat_id && self.text == text
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OutgoingSendState {
    Sending,
    Sent {
        message_id: i64,
    },
    Failed {
        reason: String,
        retryable: bool,
    },
}

impl OutgoingSendState {
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Sent { .. })
    }
}
