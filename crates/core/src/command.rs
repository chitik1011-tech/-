use serde::{Deserialize, Serialize};

use crate::{
    contacts::validate_contact_page,
    error::{CoreError, Result},
    peer::parse_peer_reference,
    search::SearchMessagesRequest,
    send::OutgoingMediaAttachment,
};

pub const MAX_CHAT_LOAD_LIMIT: usize = 500;
pub const MAX_MESSAGE_LOAD_LIMIT: usize = 200;
pub const MAX_EDIT_MESSAGE_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_REACTION_BYTES: usize = 128;
pub const MAX_BULK_MESSAGE_IDS: usize = 100;
pub const MAX_MEDIA_CAPTION_UTF16_UNITS: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CoreCommand {
    Start,
    /// Revoke the active Telegram authorization and clear account-scoped state.
    Logout,
    SetPhoneNumber(String),
    SubmitCode(String),
    SubmitPassword(String),
    LoadChats {
        limit: usize,
    },
    LoadProfile {
        peer_id: i64,
    },
    /// Load one page from the authenticated account's Telegram contacts.
    LoadContacts {
        offset: usize,
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
    /// Load the authoritative set of pinned message IDs for a chat.
    LoadPinnedMessages {
        chat_id: i64,
    },
    /// Pin a message in a chat.
    PinMessage {
        chat_id: i64,
        message_id: i64,
    },
    /// Unpin a message from a chat.
    UnpinMessage {
        chat_id: i64,
        message_id: i64,
    },
    MarkRead {
        chat_id: i64,
        max_message_id: i64,
    },
    SearchMessages(SearchMessagesRequest),
    ForwardMessages(crate::forwarding::ForwardMessagesRequest),
    /// Delete a bounded selection from one chat after Telegram accepts the request.
    DeleteMessages {
        chat_id: i64,
        message_ids: Vec<i64>,
    },
    SendText {
        chat_id: i64,
        text: String,
    },
    /// Send a user-selected one-shot geographic location. This intentionally does
    /// not participate in the text reliable-send/outbox subsystem.
    SendLocation {
        chat_id: i64,
        location: crate::model::GeoPoint,
    },
    /// Send text as a reply to a message in the same chat.
    ReplyMessage {
        chat_id: i64,
        reply_to_message_id: i64,
        text: String,
    },
    /// Reliable text send. Retrying the same client_id with the same payload is idempotent.
    SendTextReliable {
        client_id: i64,
        chat_id: i64,
        text: String,
    },
    /// Upload and send one photo, video or document through the durable send outbox.
    SendMediaReliable {
        client_id: i64,
        chat_id: i64,
        media: OutgoingMediaAttachment,
        caption: String,
        /// Runtime-assigned monotonically increasing upload attempt generation.
        #[serde(default)]
        transfer_generation: u64,
        /// Connection token attached by the core runtime to reject stale callbacks.
        #[serde(default)]
        session_generation: u64,
        #[serde(default)]
        session_epoch: u64,
    },
    /// Cancel an active outgoing upload while retaining its retryable outbox entry.
    CancelMediaSend {
        client_id: i64,
    },
    /// Edit the text of an existing Telegram message.
    EditMessage {
        chat_id: i64,
        message_id: i64,
        text: String,
    },
    /// Delete a message only for the current account where Telegram supports it.
    DeleteMessageForMe {
        chat_id: i64,
        message_id: i64,
    },
    /// Delete a message for everyone.
    DeleteMessageForEveryone {
        chat_id: i64,
        message_id: i64,
    },
    /// Submit one or more opaque Telegram option identifiers for an existing poll.
    VotePoll {
        poll_id: i64,
        chat_id: i64,
        message_id: i64,
        option_ids: Vec<Vec<u8>>,
    },
    /// Add or change the current user's reaction on a Telegram message.
    AddReaction {
        chat_id: i64,
        message_id: i64,
        reaction: crate::model::MessageReactionType,
    },
    /// Remove the current user's reaction from a Telegram message.
    RemoveReaction {
        chat_id: i64,
        message_id: i64,
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
    DownloadMedia {
        chat_id: i64,
        message_id: i64,
        attachment_index: usize,
    },
    RetryMediaDownload {
        chat_id: i64,
        message_id: i64,
        attachment_index: usize,
    },
    CancelMediaDownload {
        chat_id: i64,
        message_id: i64,
        attachment_index: usize,
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
            Self::Logout => "logout",
            Self::SetPhoneNumber(_) => "set_phone_number",
            Self::SubmitCode(_) => "submit_code",
            Self::SubmitPassword(_) => "submit_password",
            Self::LoadChats { .. } => "load_chats",
            Self::LoadProfile { .. } => "load_profile",
            Self::LoadContacts { .. } => "load_contacts",
            Self::LoadMessages { .. } => "load_messages",
            Self::LoadMessagesPage { .. } => "load_messages_page",
            Self::LoadPinnedMessages { .. } => "load_pinned_messages",
            Self::PinMessage { .. } => "pin_message",
            Self::UnpinMessage { .. } => "unpin_message",
            Self::MarkRead { .. } => "mark_read",
            Self::SearchMessages(_) => "search_messages",
            Self::ForwardMessages(_) => "forward_messages",
            Self::DeleteMessages { .. } => "delete_messages",
            Self::SendText { .. } => "send_text",
            Self::ReplyMessage { .. } => "reply_message",
            Self::SendLocation { .. } => "send_location",
            Self::SendTextReliable { .. } => "send_text_reliable",
            Self::SendMediaReliable { .. } => "send_media_reliable",
            Self::CancelMediaSend { .. } => "cancel_media_send",
            Self::EditMessage { .. } => "edit_message",
            Self::DeleteMessageForMe { .. } => "delete_message_for_me",
            Self::DeleteMessageForEveryone { .. } => "delete_message_for_everyone",
            Self::VotePoll { .. } => "vote_poll",
            Self::AddReaction { .. } => "add_reaction",
            Self::RemoveReaction { .. } => "remove_reaction",
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
            Self::DownloadMedia { .. } => "download_media",
            Self::RetryMediaDownload { .. } => "retry_media_download",
            Self::CancelMediaDownload { .. } => "cancel_media_download",
            Self::ResolvePeer { .. } => "resolve_peer",
            Self::Shutdown => "shutdown",
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Start | Self::Logout | Self::Shutdown => Ok(()),
            Self::ResolvePeer { input } => {
                parse_peer_reference(input).map(|_| ()).map_err(Into::into)
            }
            Self::DownloadMedia {
                chat_id,
                message_id,
                attachment_index,
            }
            | Self::RetryMediaDownload {
                chat_id,
                message_id,
                attachment_index,
            }
            | Self::CancelMediaDownload {
                chat_id,
                message_id,
                attachment_index,
            } => validate_media_target(*chat_id, *message_id, *attachment_index),
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
            Self::LoadProfile { peer_id } => {
                if *peer_id == 0 {
                    Err(CoreError::InvalidInput("peer id must be non-zero".into()))
                } else {
                    Ok(())
                }
            }
            Self::LoadContacts { limit, .. } => validate_contact_page(*limit),
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
            Self::LoadPinnedMessages { chat_id } => validate_chat_id(*chat_id),
            Self::PinMessage {
                chat_id,
                message_id,
            }
            | Self::UnpinMessage {
                chat_id,
                message_id,
            } => {
                validate_chat_id(*chat_id)?;
                if *message_id <= 0 {
                    return Err(CoreError::InvalidInput(
                        "message id must be positive".into(),
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
            Self::ForwardMessages(request) => request.validate(),
            Self::DeleteMessages {
                chat_id,
                message_ids,
            } => {
                validate_chat_id(*chat_id)?;
                if message_ids.is_empty() || message_ids.len() > MAX_BULK_MESSAGE_IDS {
                    return Err(CoreError::InvalidInput(format!(
                        "delete selection must contain 1..={MAX_BULK_MESSAGE_IDS} messages"
                    )));
                }
                let mut unique = std::collections::HashSet::with_capacity(message_ids.len());
                for message_id in message_ids {
                    if *message_id <= 0 || !unique.insert(*message_id) {
                        return Err(CoreError::InvalidInput(
                            "delete selection contains an invalid or duplicate message id".into(),
                        ));
                    }
                }
                Ok(())
            }
            Self::SendText { chat_id, text } => {
                validate_chat_id(*chat_id)?;
                require_non_blank("message text", text)
            }
            Self::ReplyMessage {
                chat_id,
                reply_to_message_id,
                text,
            } => {
                validate_chat_id(*chat_id)?;
                if *reply_to_message_id <= 0 {
                    return Err(CoreError::InvalidInput(
                        "reply target message id must be positive".into(),
                    ));
                }
                require_non_blank("reply text", text)
            }
            Self::SendLocation { chat_id, location } => {
                validate_chat_id(*chat_id)?;
                location.validate()
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
            Self::SendMediaReliable {
                client_id,
                chat_id,
                media,
                caption,
            } => {
                if *client_id == 0 {
                    return Err(CoreError::InvalidInput(
                        "client message id must be non-zero".into(),
                    ));
                }
                validate_chat_id(*chat_id)?;
                media.validate().map_err(CoreError::InvalidInput)?;
                if caption.encode_utf16().count() > MAX_MEDIA_CAPTION_UTF16_UNITS {
                    return Err(CoreError::InvalidInput(format!(
                        "media caption exceeds {MAX_MEDIA_CAPTION_UTF16_UNITS} UTF-16 code units"
                    )));
                }
                Ok(())
            }
            Self::CancelMediaSend { client_id } => {
                if *client_id == 0 {
                    Err(CoreError::InvalidInput("client message id must be non-zero".into()))
                } else {
                    Ok(())
                }
            }
            Self::EditMessage {
                chat_id,
                message_id,
                text,
            } => {
                validate_message_target(*chat_id, *message_id)?;
                require_non_blank("message text", text)?;
                if text.len() > MAX_EDIT_MESSAGE_TEXT_BYTES {
                    Err(CoreError::InvalidInput(format!(
                        "message text exceeds {MAX_EDIT_MESSAGE_TEXT_BYTES} UTF-8 bytes"
                    )))
                } else {
                    Ok(())
                }
            }
            Self::DeleteMessageForEveryone {
                chat_id,
                message_id,
            } => validate_message_target(*chat_id, *message_id),
            Self::DeleteMessageForMe {
                chat_id,
                message_id,
            } => validate_message_target(*chat_id, *message_id),
            Self::VotePoll {
                poll_id,
                chat_id,
                message_id,
                option_ids,
            } => {
                if *poll_id == 0 {
                    return Err(CoreError::InvalidInput("poll id must be non-zero".into()));
                }
                validate_chat_id(*chat_id)?;
                if *message_id <= 0 {
                    return Err(CoreError::InvalidInput(
                        "poll message id must be positive".into(),
                    ));
                }
                if option_ids.is_empty() {
                    return Err(CoreError::InvalidInput(
                        "poll vote must select at least one option".into(),
                    ));
                }
                if option_ids.len() > crate::poll::MAX_POLL_OPTIONS {
                    return Err(CoreError::InvalidInput(
                        "poll vote selects too many options".into(),
                    ));
                }
                let mut unique = std::collections::HashSet::with_capacity(option_ids.len());
                for option_id in option_ids {
                    if option_id.is_empty() {
                        return Err(CoreError::InvalidInput(
                            "poll option identifier must not be empty".into(),
                        ));
                    }
                    if !unique.insert(option_id.as_slice()) {
                        return Err(CoreError::InvalidInput(
                            "poll vote contains a duplicate option".into(),
                        ));
                    }
                }
                Ok(())
            }
            Self::AddReaction {
                chat_id,
                message_id,
                reaction,
            } => {
                validate_reaction_target(*chat_id, *message_id)?;
                reaction.validate()
            }
            Self::RemoveReaction {
                chat_id,
                message_id,
            } => validate_reaction_target(*chat_id, *message_id),
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

fn validate_media_target(chat_id: i64, message_id: i64, attachment_index: usize) -> Result<()> {
    validate_message_target(chat_id, message_id)?;
    if attachment_index > 31 {
        return Err(CoreError::InvalidInput(
            "attachment index must be in 0..=31".into(),
        ));
    }
    Ok(())
}

fn validate_message_target(chat_id: i64, message_id: i64) -> Result<()> {
    validate_chat_id(chat_id)?;
    if message_id <= 0 {
        return Err(CoreError::InvalidInput(
            "message id must be positive".into(),
        ));
    }
    Ok(())
}

fn validate_reaction_target(chat_id: i64, message_id: i64) -> Result<()> {
    validate_chat_id(chat_id)?;
    if message_id <= 0 {
        return Err(CoreError::InvalidInput(
            "message id must be positive".into(),
        ));
    }
    Ok(())
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
