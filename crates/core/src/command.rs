use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

pub const MAX_CHAT_LOAD_LIMIT: usize = 500;
/// Upper bound for message text accepted by the core wire API, in UTF-8 bytes.
pub const MAX_MESSAGE_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreCommand {
    Start,
    SetPhoneNumber(String),
    SubmitCode(String),
    SubmitPassword(String),
    LoadChats { limit: usize },
    SendText { chat_id: i64, text: String },
    ReplyMessage {
        chat_id: i64,
        reply_to_message_id: i64,
        text: String,
    },
    EditMessage {
        chat_id: i64,
        message_id: i64,
        text: String,
    },
    DeleteMessage { chat_id: i64, message_id: i64 },
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
            Self::SendText { .. } => "send_text",
            Self::ReplyMessage { .. } => "reply_message",
            Self::EditMessage { .. } => "edit_message",
            Self::DeleteMessage { .. } => "delete_message",
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
            Self::SendText { chat_id, text } => {
                require_non_zero("chat id", *chat_id)?;
                validate_message_text("message text", text)
            }
            Self::ReplyMessage {
                chat_id,
                reply_to_message_id,
                text,
            } => {
                require_message_ids(*chat_id, *reply_to_message_id)?;
                validate_message_text("reply text", text)
            }
            Self::EditMessage {
                chat_id,
                message_id,
                text,
            } => {
                require_message_ids(*chat_id, *message_id)?;
                validate_message_text("message text", text)
            }
            Self::DeleteMessage { chat_id, message_id } => {
                require_message_ids(*chat_id, *message_id)
            }
        }
    }
}

fn require_non_blank(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(CoreError::InvalidInput(format!("{field} is empty")))
    } else {
        Ok(())
    }
}

fn require_non_zero(field: &str, value: i64) -> Result<()> {
    if value == 0 {
        Err(CoreError::InvalidInput(format!("{field} must be non-zero")))
    } else {
        Ok(())
    }
}

fn require_message_ids(chat_id: i64, message_id: i64) -> Result<()> {
    require_non_zero("chat id", chat_id)?;
    require_non_zero("message id", message_id)
}

fn validate_message_text(field: &str, value: &str) -> Result<()> {
    require_non_blank(field, value)?;
    if value.len() > MAX_MESSAGE_TEXT_BYTES {
        return Err(CoreError::InvalidInput(format!(
            "{field} exceeds {MAX_MESSAGE_TEXT_BYTES} UTF-8 bytes"
        )));
    }
    Ok(())
}
