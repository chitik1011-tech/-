//! Typed Telegram service actions attached to message records.

use serde::{Deserialize, Serialize};

/// A Telegram chat action. This data stays separate from user-authored message text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServiceAction {
    /// A participant joined the chat.
    UserJoined { user_id: i64 },
    /// A participant left the chat.
    UserLeft { user_id: i64 },
    /// The listed participants were added to the chat.
    UsersAdded { user_ids: Vec<i64> },
    /// A participant joined through an invite link.
    JoinedByLink { inviter_id: i64 },
    /// A participant joined after an administrator approved a request.
    JoinedByRequest,
    /// The chat title changed.
    TitleChanged { title: String },
    /// The chat photo changed.
    PhotoChanged,
    /// The chat photo was removed.
    PhotoRemoved,
    /// A message was pinned; its ID may be absent from the service update.
    MessagePinned { message_id: Option<i64> },
    /// Telegram created a basic group.
    GroupCreated { title: String, user_ids: Vec<i64> },
    /// Telegram created a channel.
    ChannelCreated { title: String },
    /// A basic group migrated to a supergroup.
    GroupMigratedToSupergroup { supergroup_id: i64 },
    /// A channel was created by migrating a basic group.
    ChannelMigratedFromGroup { title: String, group_id: i64 },
    /// An unhandled Telegram service action.
    Unknown,
}

/// Structured metadata for a Telegram service message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceMessage {
    /// Telegram service action and its typed payload.
    pub action: ServiceAction,
    /// User that performed the action, when Telegram supplies it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_action_round_trips_without_message_text() {
        let service = ServiceMessage {
            action: ServiceAction::GroupMigratedToSupergroup {
                supergroup_id: -10042,
            },
            actor_id: Some(7),
        };
        let json = serde_json::to_string(&service).unwrap();
        let decoded: ServiceMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, service);
        assert!(!json.contains("message_text"));
        assert_eq!(
            decoded.action,
            ServiceAction::GroupMigratedToSupergroup {
                supergroup_id: -10042
            }
        );
    }
}
