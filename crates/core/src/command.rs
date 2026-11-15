use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

pub const MAX_CHAT_LOAD_LIMIT: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreCommand {
    Start,
    SetPhoneNumber(String),
    SubmitCode(String),
    SubmitPassword(String),
    LoadChats { limit: usize },
    /// Fetches the chat's current pinned message, if any.
    LoadPinnedMessage { chat_id: i64 },
    /// Pins a message in its chat.
    PinMessage { chat_id: i64, message_id: i64 },
    /// Removes the current pinned message from its chat.
    UnpinMessage { chat_id: i64 },
    SendText { chat_id: i64, text: String },
    Shutdown,
}

impl CoreCommand {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::SetPhoneNumber(_) => "set_phone_number",
            Self::SubmitCode(_) => "submit_code",
            Self::SubmitPassword(_) => "submit_password",
            Self::LoadChats { .. } => "load_chats",
            Self::LoadPinnedMessage { .. } => "load_pinned_message",
            Self::PinMessage { .. } => "pin_message",
            Self::UnpinMessage { .. } => "unpin_message",
            Self::SendText { .. } => "send_text",
            Self::Shutdown => "shutdown",
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Start | Self::Shutdown => Ok(()),
            Self::SetPhoneNumber(phone_number) => require_non_blank("phone number", phone_number),
            Self::SubmitCode(code) => require_non_blank("login code", code),
            Self::SubmitPassword(password) => {
                if password.is_empty() {
                    Err(CoreError::InvalidInput("password is empty".into()))
                } else {
                    Ok(())
                }
            }
            Self::LoadChats { limit } => {
                if (1..=MAX_CHAT_LOAD_LIMIT).contains(limit) {
                    Ok(())
                } else {
                    Err(CoreError::InvalidInput(format!(
                        "chat load limit must be in 1..={MAX_CHAT_LOAD_LIMIT}"
                    )))
                }
            }
            Self::LoadPinnedMessage { chat_id } | Self::UnpinMessage { chat_id } => {
                require_non_zero_chat_id(*chat_id)
            }
            Self::PinMessage {
                chat_id,
                message_id,
            } => {
                require_non_zero_chat_id(*chat_id)?;
                if *message_id <= 0 {
                    return Err(CoreError::InvalidInput("message id must be positive".into()));
                }
                Ok(())
            }
            Self::SendText { chat_id, text } => {
                if *chat_id == 0 {
                    return Err(CoreError::InvalidInput("chat id must be non-zero".into()));
                }

                require_non_blank("message text", text)
            }
        }
    }
}

fn require_non_zero_chat_id(chat_id: i64) -> Result<()> {
    if chat_id == 0 {
        Err(CoreError::InvalidInput("chat id must be non-zero".into()))
    } else {
        Ok(())
    }
}

fn require_non_blank(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(CoreError::InvalidInput(format!("{field} is empty")))
    } else {
        Ok(())
    }
}
