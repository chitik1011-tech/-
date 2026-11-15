use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// The kind of Telegram peer whose permissions were queried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatKind {
    Private,
    Group,
    Supergroup,
    Channel,
    Unknown,
}

/// Whether Telegram supplied a capability value.
///
/// Unknown means the backend did not provide an authoritative value. Callers
/// must not treat it as allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "availability", content = "value", rename_all = "snake_case")]
pub enum CapabilityValue<T> {
    Known(T),
    Unknown,
}

impl<T> Default for CapabilityValue<T> {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Actions whose availability is reported by Telegram for a particular chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAction {
    SendMessages,
    SendMedia,
    SendPolls,
    AddUsers,
    PinMessages,
    DeleteMessages,
    EditMessages,
    ManageChat,
    PostMessages,
    EditOthersMessages,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCapabilities {
    pub chat_id: i64,
    pub chat_kind: ChatKind,
    pub send_messages: CapabilityValue<bool>,
    pub send_media: CapabilityValue<bool>,
    pub send_polls: CapabilityValue<bool>,
    pub add_users: CapabilityValue<bool>,
    pub pin_messages: CapabilityValue<bool>,
    pub delete_messages: CapabilityValue<bool>,
    pub edit_messages: CapabilityValue<bool>,
    pub manage_chat: CapabilityValue<bool>,
    pub post_messages: CapabilityValue<bool>,
    pub edit_others_messages: CapabilityValue<bool>,
    /// Remaining slow-mode wait in seconds at snapshot time.
    ///
    /// Known(None) means the backend authoritatively reports that slow mode is disabled.
    /// Known(Some(0)) means no wait remains; Unknown means no authoritative remaining-time
    /// value is available. Backends must not substitute the configured slow-mode duration.
    pub slow_mode_remaining_seconds: CapabilityValue<Option<u32>>,
}

impl ChatCapabilities {
    /// Validate identifiers and concrete Telegram values before reducing a snapshot.
    pub fn validate(&self) -> Result<()> {
        if self.chat_id == 0 {
            return Err(CoreError::InvalidInput("chat id must be non-zero".into()));
        }

        Ok(())
    }

    /// Return the authoritative permission for an action without inferring defaults.
    pub fn permission(&self, action: ChatAction) -> CapabilityValue<bool> {
        match action {
            ChatAction::SendMessages => self.send_messages,
            ChatAction::SendMedia => self.send_media,
            ChatAction::SendPolls => self.send_polls,
            ChatAction::AddUsers => self.add_users,
            ChatAction::PinMessages => self.pin_messages,
            ChatAction::DeleteMessages => self.delete_messages,
            ChatAction::EditMessages => self.edit_messages,
            ChatAction::ManageChat => self.manage_chat,
            ChatAction::PostMessages => self.post_messages,
            ChatAction::EditOthersMessages => self.edit_others_messages,
        }
    }

    /// Whether an action is explicitly allowed by Telegram.
    pub fn allows(&self, action: ChatAction) -> bool {
        matches!(self.permission(action), CapabilityValue::Known(true))
    }
}

impl Default for ChatCapabilities {
    fn default() -> Self {
        Self {
            chat_id: 0,
            chat_kind: ChatKind::Unknown,
            send_messages: CapabilityValue::Unknown,
            send_media: CapabilityValue::Unknown,
            send_polls: CapabilityValue::Unknown,
            add_users: CapabilityValue::Unknown,
            pin_messages: CapabilityValue::Unknown,
            delete_messages: CapabilityValue::Unknown,
            edit_messages: CapabilityValue::Unknown,
            manage_chat: CapabilityValue::Unknown,
            post_messages: CapabilityValue::Unknown,
            edit_others_messages: CapabilityValue::Unknown,
            slow_mode_remaining_seconds: CapabilityValue::Unknown,
        }
    }
}
