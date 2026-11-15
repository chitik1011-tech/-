//! Typed Telegram service-message events and presentation hints.
//!
//! Service messages describe chat state changes; they are not user-authored text.
//! Keep this payload alongside a message record rather than synthesizing a body.

use serde::{Deserialize, Serialize};

/// Telegram action represented by a service message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServiceAction {
    /// A user joined the chat.
    UserJoined { user_id: i64 },
    /// A user left the chat.
    UserLeft { user_id: i64 },
    /// One or more users were added to the chat.
    UserAdded { user_ids: Vec<i64> },
    /// The chat title changed.
    TitleChanged { title: String },
    /// The chat photo changed or was removed. The reference is opaque and optional.
    PhotoChanged { photo_reference: Option<String> },
    /// A message was pinned.
    MessagePinned { message_id: i64 },
    /// A group was created.
    GroupCreated { title: Option<String> },
    /// A channel was created.
    ChannelCreated { title: Option<String> },
    /// A basic group migrated to a supergroup.
    GroupMigratedToSupergroup { supergroup_id: i64 },
    /// A legacy service-message marker did not contain enough information to recover its action.
    Unknown,
}

/// Non-text presentation metadata suitable for rendering a service event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ServicePresentation {
    /// Suggested icon family for the action.
    #[serde(default)]
    pub icon: ServiceIcon,
    /// Suggested visual tone.
    #[serde(default)]
    pub tone: ServiceTone,
}

impl ServicePresentation {
    /// Derive presentation hints from the typed action without creating display text.
    pub fn for_action(action: &ServiceAction) -> Self {
        let icon = match action {
            ServiceAction::UserJoined { .. }
            | ServiceAction::UserAdded { .. }
            | ServiceAction::GroupCreated { .. } => ServiceIcon::People,
            ServiceAction::UserLeft { .. } => ServiceIcon::PersonLeave,
            ServiceAction::TitleChanged { .. } => ServiceIcon::Edit,
            ServiceAction::PhotoChanged { .. } => ServiceIcon::Photo,
            ServiceAction::MessagePinned { .. } => ServiceIcon::Pin,
            ServiceAction::ChannelCreated { .. } => ServiceIcon::Channel,
            ServiceAction::GroupMigratedToSupergroup { .. } => ServiceIcon::Migration,
            ServiceAction::Unknown => ServiceIcon::Info,
        };
        Self {
            icon,
            tone: ServiceTone::Neutral,
        }
    }
}

/// Suggested icon category. Consumers remain responsible for localization and assets.
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
}

/// Suggested visual treatment for a service event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTone {
    #[default]
    Neutral,
    Highlight,
}

/// Structured service-event metadata associated with a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ServiceMessage {
    /// The Telegram action. Missing legacy data is represented as Unknown.
    pub action: ServiceAction,
    /// Actor, when supplied by Telegram.
    pub actor_id: Option<i64>,
    /// Rendering hints; no user-facing text is stored here.
    pub presentation: ServicePresentation,
}

impl Default for ServiceAction {
    fn default() -> Self {
        Self::Unknown
    }
}

impl ServiceMessage {
    /// Migrate the legacy boolean service marker without inventing an action or message text.
    pub fn from_legacy_marker(is_service_message: bool) -> Option<Self> {
        is_service_message.then(Self::default)
    }

    /// Return presentation hints derived from the action, ignoring stale serialized hints.
    pub fn canonical_presentation(&self) -> ServicePresentation {
        ServicePresentation::for_action(&self.action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_round_trips_as_typed_data_with_presentation() {
        let action = ServiceAction::GroupMigratedToSupergroup { supergroup_id: -10042 };
        let event = ServiceMessage {
            action: action.clone(),
            actor_id: Some(7),
            presentation: ServicePresentation::for_action(&action),
        };

        let json = serde_json::to_string(&event).expect("serialize service event");
        let decoded: ServiceMessage =
            serde_json::from_str(&json).expect("deserialize service event");
        assert_eq!(decoded, event);
        assert_eq!(decoded.canonical_presentation().icon, ServiceIcon::Migration);
        assert!(!json.contains("text"));
    }

    #[test]
    fn legacy_payload_defaults_missing_action_actor_and_presentation() {
        let decoded: ServiceMessage =
            serde_json::from_str("{}").expect("deserialize legacy payload");

        assert_eq!(decoded.action, ServiceAction::Unknown);
        assert_eq!(decoded.actor_id, None);
        assert_eq!(decoded.presentation, ServicePresentation::default());
    }

    #[test]
    fn legacy_marker_migrates_without_fabricated_text() {
        assert_eq!(ServiceMessage::from_legacy_marker(false), None);
        let migrated = ServiceMessage::from_legacy_marker(true).expect("service marker");
        assert_eq!(migrated.action, ServiceAction::Unknown);
        assert_eq!(migrated.actor_id, None);
        assert_eq!(migrated.presentation.icon, ServiceIcon::Info);
    }

    #[test]
    fn actions_have_stable_presentation_categories() {
        let cases = [
            (
                ServiceAction::UserJoined { user_id: 1 },
                ServiceIcon::People,
            ),
            (
                ServiceAction::UserLeft { user_id: 1 },
                ServiceIcon::PersonLeave,
            ),
            (
                ServiceAction::UserAdded {
                    user_ids: vec![1, 2],
                },
                ServiceIcon::People,
            ),
            (
                ServiceAction::TitleChanged {
                    title: "Team".into(),
                },
                ServiceIcon::Edit,
            ),
            (
                ServiceAction::PhotoChanged {
                    photo_reference: None,
                },
                ServiceIcon::Photo,
            ),
            (
                ServiceAction::MessagePinned { message_id: 4 },
                ServiceIcon::Pin,
            ),
            (
                ServiceAction::GroupCreated { title: None },
                ServiceIcon::People,
            ),
            (
                ServiceAction::ChannelCreated { title: None },
                ServiceIcon::Channel,
            ),
            (
                ServiceAction::GroupMigratedToSupergroup { supergroup_id: -100 },
                ServiceIcon::Migration,
            ),
        ];

        for (action, icon) in cases {
            assert_eq!(ServicePresentation::for_action(&action).icon, icon);
        }
    }
}
