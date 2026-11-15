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
    /// Load one page; `cursor: None` requests the first page.
    LoadChatsPage { cursor: Option<String>, limit: usize },
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
            Self::LoadChatsPage { .. } => "load_chats_page",
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
            Self::LoadChats { limit } | Self::LoadChatsPage { limit, .. } => {
                validate_chat_load_limit(*limit)
            }
            Self::LoadChatsPage { cursor: Some(cursor), .. } if cursor.trim().is_empty() => {
                Err(CoreError::InvalidInput("chat page cursor is empty".into()))
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

fn require_non_blank(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(CoreError::InvalidInput(format!("{field} is empty")))
    } else {
        Ok(())
    }
}

fn validate_chat_load_limit(limit: usize) -> Result<()> {
    if (1..=MAX_CHAT_LOAD_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(CoreError::InvalidInput(format!(
            "chat load limit must be in 1..={MAX_CHAT_LOAD_LIMIT}"
        )))
    }
}
