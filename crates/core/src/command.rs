use serde::{Deserialize, Serialize};

use crate::{
    error::{CoreError, Result},
    peer::parse_peer_reference,
    search::SearchMessagesRequest,
};

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
    /// Fetch one page older than `before_message_id` (exclusive).
    LoadMessagesPage {
        chat_id: i64,
        before_message_id: Option<i64>,
        limit: usize,
    },
    MarkRead {
        chat_id: i64,
        max_message_id: i64,
    },
    SearchMessages(SearchMessagesRequest),
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
    ArchiveChat {
        chat_id: i64,
    },
    UnarchiveChat {
        chat_id: i64,
    },
    MuteChat {
        chat_id: i64,
    },
    UnmuteChat {
        chat_id: i64,
    },
    PinChat {
        chat_id: i64,
    },
    UnpinChat {
        chat_id: i64,
    },
    MarkChatUnread {
        chat_id: i64,
    },
    MarkChatRead {
        chat_id: i64,
    },
    /// Save or replace the current draft for a chat.
    UpdateDraft(crate::model::Draft),
    /// Clear a draft. Guarded clears compare its timestamp and full captured identity.
    ClearDraft {
        chat_id: i64,
        expected_updated_at_unix: Option<i64>,
        #[serde(default)]
        expected_draft: Option<crate::model::Draft>,
    },
    ResolvePeer {
        input: String,
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
            Self::LoadMessagesPage { .. } => "load_messages_page",
            Self::MarkRead { .. } => "mark_read",
            Self::SearchMessages(_) => "search_messages",
            Self::SendText { .. } => "send_text",
            Self::SendTextReliable { .. } => "send_text_reliable",
            Self::ArchiveChat { .. } => "archive_chat",
            Self::UnarchiveChat { .. } => "unarchive_chat",
            Self::MuteChat { .. } => "mute_chat",
            Self::UnmuteChat { .. } => "unmute_chat",
            Self::PinChat { .. } => "pin_chat",
            Self::UnpinChat { .. } => "unpin_chat",
            Self::MarkChatUnread { .. } => "mark_chat_unread",
            Self::MarkChatRead { .. } => "mark_chat_read",
            Self::UpdateDraft(_) => "update_draft",
            Self::ClearDraft { .. } => "clear_draft",
            Self::ResolvePeer { .. } => "resolve_peer",
            Self::Shutdown => "shutdown",
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Start | Self::Shutdown => Ok(()),
            Self::ResolvePeer { input } => {
                parse_peer_reference(input).map(|_| ()).map_err(Into::into)
            }
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
            Self::LoadMessagesPage {
                chat_id,
                before_message_id,
                limit,
            } => {
                validate_chat_id(*chat_id)?;
                validate_limit("message page", *limit, MAX_MESSAGE_LOAD_LIMIT)?;
                if before_message_id.is_some_and(|message_id| message_id <= 0) {
                    return Err(CoreError::InvalidInput(
                        "message page cursor must be positive".into(),
                    ));
                }
                Ok(())
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
            Self::SearchMessages(request) => request.validate(),
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
            Self::ArchiveChat { chat_id }
            | Self::UnarchiveChat { chat_id }
            | Self::MuteChat { chat_id }
            | Self::UnmuteChat { chat_id }
            | Self::PinChat { chat_id }
            | Self::UnpinChat { chat_id }
            | Self::MarkChatUnread { chat_id }
            | Self::MarkChatRead { chat_id } => validate_chat_id(*chat_id),
            Self::UpdateDraft(draft) => draft.validate(),
            Self::ClearDraft {
                chat_id,
                expected_updated_at_unix,
                expected_draft,
            } => {
                validate_chat_id(*chat_id)?;
                if expected_updated_at_unix.is_some_and(|timestamp| timestamp < 0) {
                    Err(CoreError::InvalidInput(
                        "draft timestamp must be non-negative".into(),
                    ))
                } else if expected_updated_at_unix.is_some() && expected_draft.is_none() {
                    Err(CoreError::InvalidInput(
                        "timestamp-guarded clear requires the expected draft".into(),
                    ))
                } else if let Some(draft) = expected_draft {
                    draft.validate()?;
                    if draft.chat_id != *chat_id
                        || expected_updated_at_unix
                            .is_some_and(|timestamp| timestamp != draft.updated_at_unix)
                    {
                        return Err(CoreError::InvalidInput(
                            "expected draft does not match clear target".into(),
                        ));
                    }
                    Ok(())
                } else {
                    Ok(())
                }
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
