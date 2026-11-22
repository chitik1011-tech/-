use serde::{Deserialize, Serialize};

/// Telegram peer category for which permissions were reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChatKind {
    Private,
    Group,
    Supergroup,
    Channel,
    #[default]
    Unknown,
}

/// A server-reported permission. Unknown values never authorize an action.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAction {
    SendMessages,
    SendMedia,
    SendPolls,
    SendStickers,
    PinMessages,
    DeleteMessages,
    EditMessages,
    ForwardMessages,
    InviteUsers,
    BanUsers,
    ManageChat,
    PostMessages,
    EditOthersMessages,
}

/// One authoritative Telegram permission snapshot for one chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ChatCapabilities {
    pub chat_id: i64,
    pub chat_kind: ChatKind,
    pub can_send_messages: CapabilityValue<bool>,
    /// Rust-derived permission to send at the snapshot time, including slow mode.
    pub can_send_now: CapabilityValue<bool>,
    pub can_send_media: CapabilityValue<bool>,
    pub can_send_polls: CapabilityValue<bool>,
    pub can_send_stickers: CapabilityValue<bool>,
    pub can_pin_messages: CapabilityValue<bool>,
    pub can_delete_messages: CapabilityValue<bool>,
    pub can_edit_messages: CapabilityValue<bool>,
    pub can_forward_messages: CapabilityValue<bool>,
    pub can_invite_users: CapabilityValue<bool>,
    pub can_ban_users: CapabilityValue<bool>,
    pub can_manage_chat: CapabilityValue<bool>,
    pub can_post_messages: CapabilityValue<bool>,
    pub can_edit_others_messages: CapabilityValue<bool>,
    pub read_only: CapabilityValue<bool>,
    pub restricted: CapabilityValue<bool>,
    pub banned: CapabilityValue<bool>,
    pub slow_mode_enabled: CapabilityValue<bool>,
    /// Remaining server-reported slow-mode wait at snapshot time.
    /// `Known(None)` means no wait is pending; unknown is never treated as zero.
    pub slow_mode_remaining_seconds: CapabilityValue<Option<u32>>,
}

impl ChatCapabilities {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.chat_id == 0 {
            return Err("chat capability id must be non-zero");
        }
        if let CapabilityValue::Known(Some(seconds)) = self.slow_mode_remaining_seconds {
            if seconds > 86_400 {
                return Err("slow-mode wait exceeds one day");
            }
        }
        if self.can_send_now != self.computed_can_send_now() {
            return Err("derived send-now capability is inconsistent");
        }
        Ok(())
    }

    pub fn permission(&self, action: ChatAction) -> CapabilityValue<bool> {
        match action {
            ChatAction::SendMessages => self.can_send_messages,
            ChatAction::SendMedia => self.can_send_media,
            ChatAction::SendPolls => self.can_send_polls,
            ChatAction::SendStickers => self.can_send_stickers,
            ChatAction::PinMessages => self.can_pin_messages,
            ChatAction::DeleteMessages => self.can_delete_messages,
            ChatAction::EditMessages => self.can_edit_messages,
            ChatAction::ForwardMessages => self.can_forward_messages,
            ChatAction::InviteUsers => self.can_invite_users,
            ChatAction::BanUsers => self.can_ban_users,
            ChatAction::ManageChat => self.can_manage_chat,
            ChatAction::PostMessages => self.can_post_messages,
            ChatAction::EditOthersMessages => self.can_edit_others_messages,
        }
    }

    /// True only when Telegram explicitly permits sending now, including slow mode.
    pub fn can_send_now(&self) -> CapabilityValue<bool> {
        self.can_send_now
    }

    pub fn refresh_derived(&mut self) {
        self.can_send_now = self.computed_can_send_now();
    }

    fn computed_can_send_now(&self) -> CapabilityValue<bool> {
        if self.can_send_messages == CapabilityValue::Known(false)
            || self.read_only == CapabilityValue::Known(true)
            || self.restricted == CapabilityValue::Known(true)
            || self.banned == CapabilityValue::Known(true)
        {
            return CapabilityValue::Known(false);
        }
        if self.can_send_messages != CapabilityValue::Known(true) {
            return CapabilityValue::Unknown;
        }
        match (self.slow_mode_enabled, self.slow_mode_remaining_seconds) {
            (CapabilityValue::Known(false), _) => CapabilityValue::Known(true),
            (CapabilityValue::Known(true), CapabilityValue::Known(Some(0)))
            | (CapabilityValue::Known(true), CapabilityValue::Known(None)) => {
                CapabilityValue::Known(true)
            }
            (CapabilityValue::Known(true), CapabilityValue::Known(Some(_))) => {
                CapabilityValue::Known(false)
            }
            _ => CapabilityValue::Unknown,
        }
    }

    pub fn allows(&self, action: ChatAction) -> bool {
        self.permission(action) == CapabilityValue::Known(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_never_authorizes_and_does_not_become_zero_slow_mode() {
        let capabilities = ChatCapabilities {
            chat_id: 7,
            ..ChatCapabilities::default()
        };
        assert!(!capabilities.allows(ChatAction::SendMessages));
        assert_eq!(capabilities.can_send_now(), CapabilityValue::Unknown);
    }

    #[test]
    fn slow_mode_blocks_until_authoritative_remaining_time_expires() {
        let mut capabilities = ChatCapabilities {
            chat_id: 7,
            can_send_messages: CapabilityValue::Known(true),
            slow_mode_enabled: CapabilityValue::Known(true),
            slow_mode_remaining_seconds: CapabilityValue::Known(Some(12)),
            ..ChatCapabilities::default()
        };
        capabilities.refresh_derived();
        assert_eq!(capabilities.can_send_now(), CapabilityValue::Known(false));
        capabilities.slow_mode_remaining_seconds = CapabilityValue::Known(Some(0));
        capabilities.refresh_derived();
        assert_eq!(capabilities.can_send_now(), CapabilityValue::Known(true));
        capabilities.read_only = CapabilityValue::Known(true);
        capabilities.refresh_derived();
        assert_eq!(capabilities.can_send_now(), CapabilityValue::Known(false));
    }

    #[test]
    fn wire_represents_unknown_explicitly() {
        let value = serde_json::to_value(CapabilityValue::<bool>::Unknown).unwrap();
        assert_eq!(value["availability"], "unknown");
        let known = serde_json::to_value(CapabilityValue::Known(true)).unwrap();
        assert_eq!(known["value"], true);
    }
}
