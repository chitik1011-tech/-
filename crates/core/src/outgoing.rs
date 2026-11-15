//! Rust-owned lifecycle for outgoing text messages.
//!
//! The UI creates a stable client ID for one logical message and reuses it when retrying.
//! A successful backend result is the acceptance boundary for clearing the UI draft. Retries
//! after an interrupted/ambiguous network result can still duplicate a Telegram send unless
//! the concrete backend also uses this ID as its protocol idempotency key.

use serde::{Deserialize, Serialize};

use crate::{
    command::CoreCommand,
    error::{CoreError, Result},
};

pub const MAX_CLIENT_MESSAGE_ID_LENGTH: usize = 128;

/// A request routed through core without exposing lifecycle-only commands to Telegram backends.
///
/// Replaying an identical request with the same client ID retries only if its prior state is
/// failed. Requests for sending or sent IDs are idempotent no-ops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum OutgoingMessageCommand {
    SendTextWithId {
        client_id: String,
        chat_id: i64,
        text: String,
    },
}

impl OutgoingMessageCommand {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::SendTextWithId {
                client_id,
                chat_id,
                text,
            } => {
                validate_client_id(client_id)?;
                CoreCommand::SendText {
                    chat_id: *chat_id,
                    text: text.clone(),
                }
                .validate()
            }
        }
    }

    pub fn client_id(&self) -> &str {
        match self {
            Self::SendTextWithId { client_id, .. } => client_id,
        }
    }

    pub fn chat_id(&self) -> i64 {
        match self {
            Self::SendTextWithId { chat_id, .. } => *chat_id,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::SendTextWithId { text, .. } => text,
        }
    }

    pub(crate) fn into_request(self) -> Result<OutgoingMessage> {
        self.validate()?;
        match self {
            Self::SendTextWithId {
                client_id,
                chat_id,
                text,
            } => Ok(OutgoingMessage {
                client_id,
                chat_id,
                text,
                status: OutgoingMessageStatus::Draft,
                server_message_id: None,
                error: None,
            }),
        }
    }
}

/// Observable send lifecycle state, keyed by the caller-provided stable client ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutgoingMessageStatus {
    Draft,
    Sending,
    Sent,
    Failed,
}

/// A pending or completed outgoing message owned by core.
///
/// The client ID identifies this logical send across retries. It is deliberately distinct from
/// Telegram's server-assigned message ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingMessage {
    pub client_id: String,
    pub chat_id: i64,
    pub text: String,
    pub status: OutgoingMessageStatus,
    pub server_message_id: Option<i64>,
    pub error: Option<String>,
}

impl OutgoingMessageStatus {
    pub(crate) const fn may_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Sending)
                | (Self::Sending, Self::Sent)
                | (Self::Sending, Self::Failed)
                | (Self::Failed, Self::Sending)
        )
    }
}

fn validate_client_id(client_id: &str) -> Result<()> {
    let valid = !client_id.is_empty()
        && client_id.len() <= MAX_CLIENT_MESSAGE_ID_LENGTH
        && client_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'));
    if valid {
        Ok(())
    } else {
        Err(CoreError::InvalidInput(format!(
            "client message id must contain 1..={MAX_CLIENT_MESSAGE_ID_LENGTH} ASCII letters, digits, '.', '_', ':', or '-'"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::OutgoingMessageCommand;

    #[test]
    fn validates_stable_client_id_and_send_payload() {
        let valid = OutgoingMessageCommand::SendTextWithId {
            client_id: "msg-7d2a".into(),
            chat_id: -42,
            text: " hello ".into(),
        };
        assert!(valid.validate().is_ok());

        for client_id in ["", "contains spaces", "slash/invalid"] {
            let invalid = OutgoingMessageCommand::SendTextWithId {
                client_id: client_id.into(),
                chat_id: 42,
                text: "hello".into(),
            };
            assert!(invalid.validate().is_err());
        }
    }
}
