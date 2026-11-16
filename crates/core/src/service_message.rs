//! Structured Telegram service actions and stable UI presentation hints.
//!
//! The action is the source of truth. Presentation hints carry no localized text,
//! so Android can render the event in the user's locale without changing stored data.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServiceAction {
    /// A user joined without being explicitly added to the chat.
    UserJoined { user_id: Option<i64>, inviter_id: Option<i64> },
    /// The actor left the group.
    UserLeft { user_id: i64 },
    /// A different participant was removed by the actor.
    UserRemoved { user_id: i64 },
    /// One or more users were added to the chat.
    UserAdded { user_ids: Vec<i64> },
    /// The chat title changed.
    TitleChanged { title: String },
    /// The chat photo changed; photo id is absent when Telegram did not expose one.
    PhotoChanged { photo_id: Option<i64> },
    /// The chat photo was removed.
    PhotoRemoved,
    /// A message was pinned.
    MessagePinned { message_id: Option<i64> },
    /// A basic group was created.
    GroupCreated { title: String, user_ids: Vec<i64> },
    /// A channel was created.
    ChannelCreated { title: String },
    /// A basic group was migrated into a supergroup.
    GroupMigratedToSupergroup { supergroup_id: i64 },
    /// A supergroup or channel originated from a basic group.
    ChannelMigratedFromGroup { group_id: i64, title: String },
    /// Chat history was cleared.
    HistoryCleared,
    /// Telegram supplied a service action not yet represented by this model.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ServiceIcon {
    #[default]
    Info,
    People,
    PersonLeave,
    Edit,
    Photo,
    Pin,
    Channel,
    Migration,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTone {
    #[default]
    Neutral,
    Highlight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ServicePresentation {
    pub icon: ServiceIcon,
    pub tone: ServiceTone,
}

impl ServicePresentation {
    pub fn for_action(action: &ServiceAction) -> Self {
        let icon = match action {
            ServiceAction::UserJoined { .. }
            | ServiceAction::UserAdded { .. }
            | ServiceAction::UserRemoved { .. }
            | ServiceAction::GroupCreated { .. } => ServiceIcon::People,
            ServiceAction::UserLeft { .. } => ServiceIcon::PersonLeave,
            ServiceAction::TitleChanged { .. } => ServiceIcon::Edit,
            ServiceAction::PhotoChanged { .. } | ServiceAction::PhotoRemoved => ServiceIcon::Photo,
            ServiceAction::MessagePinned { .. } => ServiceIcon::Pin,
            ServiceAction::ChannelCreated { .. } => ServiceIcon::Channel,
            ServiceAction::GroupMigratedToSupergroup { .. }
            | ServiceAction::ChannelMigratedFromGroup { .. } => ServiceIcon::Migration,
            ServiceAction::HistoryCleared => ServiceIcon::Clear,
            ServiceAction::Unknown => ServiceIcon::Info,
        };
        Self { icon, tone: ServiceTone::Neutral }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ServiceMessage {
    pub action: ServiceAction,
    pub actor_id: Option<i64>,
    pub presentation: ServicePresentation,
}

impl Default for ServiceAction {
    fn default() -> Self { Self::Unknown }
}

impl ServiceMessage {
    /// Create metadata and derive canonical presentation hints for its action.
    pub fn new(action: ServiceAction, actor_id: Option<i64>) -> Self {
        let presentation = ServicePresentation::for_action(&action);
        Self { action, actor_id, presentation }
    }

    /// Convert a legacy service marker to an unknown typed action without inventing details.
    pub fn from_legacy_marker(is_service_message: bool) -> Option<Self> {
        is_service_message.then(|| Self::new(ServiceAction::Unknown, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_actions_round_trip_as_structured_metadata_without_text() {
        let actions = [
            ServiceAction::UserJoined { user_id: 1, inviter_id: Some(2) },
            ServiceAction::UserLeft { user_id: 1 },
            ServiceAction::UserRemoved { user_id: 1 },
            ServiceAction::UserAdded { user_ids: vec![1, 2] },
            ServiceAction::TitleChanged { title: "Group".into() },
            ServiceAction::PhotoChanged { photo_id: Some(42) },
            ServiceAction::PhotoRemoved,
            ServiceAction::MessagePinned,
            ServiceAction::GroupCreated { title: "Group".into(), user_ids: vec![1] },
            ServiceAction::ChannelCreated { title: "Channel".into() },
            ServiceAction::GroupMigratedToSupergroup { supergroup_id: -1001 },
            ServiceAction::ChannelMigratedFromGroup { group_id: 1, title: "Group".into() },
            ServiceAction::HistoryCleared,
            ServiceAction::Unknown,
        ];
        for action in actions {
            let message = ServiceMessage::new(action, Some(7));
            let json = serde_json::to_string(&message).unwrap();
            assert!(!json.contains("text"));
            assert_eq!(serde_json::from_str::<ServiceMessage>(&json).unwrap(), message);
        }
    }

    #[test]
    fn missing_and_malformed_action_payloads_are_safe() {
        assert_eq!(serde_json::from_str::<ServiceMessage>("{}").unwrap().action, ServiceAction::Unknown);
        assert!(serde_json::from_str::<ServiceMessage>(r#"{"action":{"type":"user_added","user_ids":"bad"}}"#).is_err());
        assert_eq!(ServiceMessage::from_legacy_marker(true).unwrap().action, ServiceAction::Unknown);
        assert!(ServiceMessage::from_legacy_marker(false).is_none());
    }
}
