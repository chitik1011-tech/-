use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorizationState {
    WaitingForPhoneNumber,
    WaitingForCode,
    WaitingForPassword,
    Ready,
    LoggingOut,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSummary {
    pub id: i64,
    pub title: String,
    pub unread_count: u32,
    pub last_message: Option<MessageSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: i64,
    pub chat_id: i64,
    pub sender_id: i64,
    pub text: String,
    pub timestamp_unix: i64,
    pub outgoing: bool,
    /// Original sender and source context for forwarded messages.
    #[serde(default)]
    pub forwarded_from: Option<ForwardedFrom>,
    /// Whether the message has been edited.
    #[serde(default)]
    pub edited: bool,
    /// Unix timestamp of the most recent edit, when known.
    #[serde(default)]
    pub edit_timestamp_unix: Option<i64>,
    /// Whether this message represents a Telegram service action.
    #[serde(default)]
    pub service_message: bool,
    /// Caption attached to media, kept separate from ordinary message text.
    #[serde(default)]
    pub media_caption: Option<String>,
    /// Target of this message when it is a reply.
    #[serde(default)]
    pub reply_to: Option<ReplyMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ForwardedFrom {
    /// Sender ID in the original conversation, when Telegram exposes it.
    #[serde(default)]
    pub sender_id: Option<i64>,
    /// Original chat ID for a forwarded chat message.
    #[serde(default)]
    pub chat_id: Option<i64>,
    /// Original channel ID for a forwarded channel post.
    #[serde(default)]
    pub channel_id: Option<i64>,
    /// Original message timestamp in Unix seconds, when available.
    #[serde(default)]
    pub timestamp_unix: Option<i64>,
    /// Display name captured from the original sender, when available.
    #[serde(default)]
    pub sender_display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyMetadata {
    /// ID of the message being replied to.
    pub message_id: i64,
    /// Chat containing the replied-to message when it differs or is known.
    #[serde(default)]
    pub chat_id: Option<i64>,
    /// Sender of the replied-to message, when known.
    #[serde(default)]
    pub sender_id: Option<i64>,
    /// Display name of the replied-to sender, when available.
    #[serde(default)]
    pub sender_display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    pub authorization: AuthorizationState,
    pub chats: Vec<ChatSummary>,
}

impl Default for AccountState {
    fn default() -> Self {
        Self {
            authorization: AuthorizationState::Closed,
            chats: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CoreState;

    #[test]
    fn legacy_persisted_state_defaults_new_message_metadata() {
        let legacy = r#"{
            "running": true,
            "account": {
                "authorization": "Ready",
                "chats": [{
                    "id": 3,
                    "title": "chat",
                    "unread_count": 1,
                    "last_message": {
                        "id": 7,
                        "chat_id": 3,
                        "sender_id": 9,
                        "text": "old message",
                        "timestamp_unix": 100,
                        "outgoing": false
                    }
                }]
            },
            "messages": {
                "3": [{
                    "id": 7,
                    "chat_id": 3,
                    "sender_id": 9,
                    "text": "old message",
                    "timestamp_unix": 100,
                    "outgoing": false
                }]
            },
            "last_error": null
        }"#;

        let state: CoreState =
            serde_json::from_str(legacy).expect("legacy state should deserialize");
        let message = &state.messages_for_chat(3)[0];
        assert_eq!(message.forwarded_from, None);
        assert!(!message.edited);
        assert_eq!(message.edit_timestamp_unix, None);
        assert_eq!(message.reply_to, None);
        assert!(!message.service_message);
        assert_eq!(message.media_caption, None);
        assert_eq!(
            state.chat(3).and_then(|chat| chat.last_message.as_ref()),
            Some(message)
        );
    }

    #[test]
    fn message_metadata_round_trips_through_json() {
        let message = MessageSummary {
            id: 11,
            chat_id: 3,
            sender_id: 9,
            text: "forwarded reply".into(),
            timestamp_unix: 200,
            outgoing: false,
            service_message: true,
            media_caption: Some("photo caption".into()),
            forwarded_from: Some(ForwardedFrom {
                sender_id: Some(12),
                chat_id: Some(4),
                channel_id: Some(5),
                timestamp_unix: Some(150),
                sender_display_name: Some("Original author".into()),
            }),
            edited: true,
            edit_timestamp_unix: Some(210),
            reply_to: Some(ReplyMetadata {
                message_id: 8,
                chat_id: Some(3),
                sender_id: Some(10),
                sender_display_name: Some("Reply target".into()),
            }),
        };

        let encoded = serde_json::to_string(&message).expect("message should serialize");
        let decoded: MessageSummary =
            serde_json::from_str(&encoded).expect("message should deserialize");

        assert_eq!(decoded, message);
    }
}
