use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

pub const MAX_CHAT_LOAD_LIMIT: usize = 500;
pub const MAX_MESSAGE_LOAD_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreCommand {
    Start,
    SetPhoneNumber(String),
    SubmitCode(String),
    SubmitPassword(String),
    LoadChats {
        limit: usize,
    },
    LoadMessages {
        chat_id: i64,
        limit: usize,
    },
    MarkRead {
        chat_id: i64,
        max_message_id: i64,
    },
    SendText {
        chat_id: i64,
        text: String,
    },
    /// Reliable text send. Retrying the same client_id with the same payload is idempotent.
    SendTextReliable {
        client_id: i64,
        chat_id: i64,
        text: String,
    },
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
            Self::LoadMessages { .. } => "load_messages",
            Self::MarkRead { .. } => "mark_read",
            Self::SendText { .. } => "send_text",
            Self::SendTextReliable { .. } => "send_text_reliable",
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
            Self::LoadChats { limit } => validate_limit("chat load", *limit, MAX_CHAT_LOAD_LIMIT),
            Self::LoadMessages { chat_id, limit } => {
                validate_chat_id(*chat_id)?;
                validate_limit("message load", *limit, MAX_MESSAGE_LOAD_LIMIT)
            }
            Self::MarkRead {
                chat_id,
                max_message_id,
            } => {
                validate_chat_id(*chat_id)?;
                if *max_message_id <= 0 {
                    Err(CoreError::InvalidInput(
                        "max message id must be positive".into(),
                    ))
                } else {
                    Ok(())
                }
            }
            Self::SendText { chat_id, text } => {
                validate_chat_id(*chat_id)?;
                require_non_blank("message text", text)
            }
            Self::SendTextReliable {
                client_id,
                chat_id,
                text,
            } => {
                if *client_id == 0 {
                    return Err(CoreError::InvalidInput(
                        "client message id must be non-zero".into(),
                    ));
                }
                validate_chat_id(*chat_id)?;
                require_non_blank("message text", text)
            }
        }
    }
}

fn validate_chat_id(chat_id: i64) -> Result<()> {
    if chat_id == 0 {
        Err(CoreError::InvalidInput("chat id must be non-zero".into()))
    } else {
        Ok(())
    }
}

fn validate_limit(field: &str, limit: usize, maximum: usize) -> Result<()> {
    if (1..=maximum).contains(&limit) {
        Ok(())
    } else {
        Err(CoreError::InvalidInput(format!(
            "{field} limit must be in 1..={maximum}"
        )))
    }
}

fn require_non_blank(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(CoreError::InvalidInput(format!("{field} is empty")))
    } else {
        Ok(())
    }
}
