use serde::{Deserialize, Serialize};

use crate::{
    error::{CoreError, Result},
    search::SearchMessagesRequest,
};

pub const MAX_CHAT_LOAD_LIMIT: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreCommand {
    Start,
    SetPhoneNumber(String),
    SubmitCode(String),
    SubmitPassword(String),
    LoadChats { limit: usize },
    SendText { chat_id: i64, text: String },
    SearchMessages(SearchMessagesRequest),
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
            Self::SearchMessages(_) => "search_messages",
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
            Self::SearchMessages(request) => request.validate(),
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
