use serde::{Deserialize, Serialize};
use std::fmt;

use crate::error::CoreError;

pub const MAX_PEER_REFERENCE_LENGTH: usize = 2_048;

/// A syntactically validated target. Parsing is deterministic and never performs I/O.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum PeerReference {
    Username {
        username: String,
    },
    /// Stable user identifier from a `tg://user?id=…` deep link.
    NumericUserId {
        user_id: i64,
    },
    MessageLink(MessageLink),
}

/// A parsed public or private Telegram message link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageLink {
    pub peer: MessagePeer,
    pub message_id: i64,
    #[serde(default)]
    pub metadata: MessageLinkMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum MessagePeer {
    Username {
        username: String,
    },
    /// Telegram's /c/ path exposes an internal channel number, not a public chat ID.
    InternalChannelId {
        channel_id: i64,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageLinkMetadata {
    #[serde(default)]
    pub single: bool,
    #[serde(default)]
    pub thread_id: Option<i64>,
    #[serde(default)]
    pub comment_id: Option<i64>,
}

/// The result returned by a Telegram backend after attempting actual resolution.
///
/// These outcomes describe identity only. They do not grant or imply permission to read,
/// join, message, or otherwise interact with the peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum PeerResolution {
    User {
        user_id: i64,
        username: Option<String>,
    },
    Bot {
        user_id: i64,
        username: Option<String>,
    },
    Group {
        chat_id: i64,
        title: Option<String>,
    },
    Supergroup {
        chat_id: i64,
        title: Option<String>,
    },
    Channel {
        chat_id: i64,
        title: Option<String>,
    },
    MessageLink(MessageLinkResolution),
    Unknown {
        reason: String,
    },
    NotFound,
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageLinkResolution {
    pub chat_id: i64,
    pub message_id: i64,
    #[serde(default)]
    pub metadata: MessageLinkMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerParseError {
    Empty,
    InvalidUsername,
    InvalidUrl,
    UnsupportedUrl,
    InvalidNumericId,
    InvalidMessageId,
    InvalidMetadata,
}

impl fmt::Display for PeerParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "peer reference is empty",
            Self::InvalidUsername => "username must be 5-32 ASCII letters, digits, or underscores and start with a letter",
            Self::InvalidUrl => "Telegram link is malformed",
            Self::UnsupportedUrl => "URL is not a supported Telegram peer link",
            Self::InvalidNumericId => "numeric user ID must be a positive signed 64-bit integer",
            Self::InvalidMessageId => "message ID must be a positive signed 64-bit integer",
            Self::InvalidMetadata => "message-link metadata is malformed or duplicated",
        };
        f.write_str(message)
    }
}

impl std::error::Error for PeerParseError {}

impl From<PeerParseError> for CoreError {
    fn from(error: PeerParseError) -> Self {
        CoreError::InvalidInput(error.to_string())
    }
}

/// Parse a full `@username`, exact t.me URL, or stable `tg://user?id=…` reference.
///
/// This function is pure: it validates and extracts data only; resolution belongs to
/// `TelegramBackend::resolve_peer`.
pub fn parse_peer_reference(input: &str) -> Result<PeerReference, PeerParseError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(PeerParseError::Empty);
    }
    if input.len() > MAX_PEER_REFERENCE_LENGTH {
        return Err(PeerParseError::InvalidUrl);
    }

    if let Some(username) = input.strip_prefix('@') {
        validate_username(username)?;
        return Ok(PeerReference::Username {
            username: username.to_ascii_lowercase(),
        });
    }

    if input.starts_with("tg://") {
        return parse_tg_uri(input);
    }

    if input.starts_with("https://") || input.starts_with("http://") {
        return parse_tme_url(input);
    }

    Err(PeerParseError::UnsupportedUrl)
}

fn validate_username(username: &str) -> Result<(), PeerParseError> {
    let bytes = username.as_bytes();
    if !(5..=32).contains(&bytes.len())
        || !bytes[0].is_ascii_alphabetic()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return Err(PeerParseError::InvalidUsername);
    }
    Ok(())
}

fn parse_tg_uri(input: &str) -> Result<PeerReference, PeerParseError> {
    let rest = input
        .strip_prefix("tg://")
        .ok_or(PeerParseError::InvalidUrl)?;
    let (route, query) = rest.split_once('?').ok_or(PeerParseError::InvalidUrl)?;
    if route != "user" {
        return Err(PeerParseError::UnsupportedUrl);
    }
    let mut id = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or(PeerParseError::InvalidUrl)?;
        if key == "id" {
            if id.is_some() {
                return Err(PeerParseError::InvalidUrl);
            }
            id = Some(parse_positive_i64(value).ok_or(PeerParseError::InvalidNumericId)?);
        }
    }
    let user_id = id.ok_or(PeerParseError::InvalidNumericId)?;
    Ok(PeerReference::NumericUserId { user_id })
}

fn parse_tme_url(input: &str) -> Result<PeerReference, PeerParseError> {
    let (scheme, after_scheme) = input.split_once("://").ok_or(PeerParseError::InvalidUrl)?;
    if scheme != "https" && scheme != "http" {
        return Err(PeerParseError::UnsupportedUrl);
    }

    let authority_end = after_scheme
        .find(|ch| ch == '/' || ch == '?' || ch == '#')
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    if authority.contains('@') || authority.contains(':') || authority.is_empty() {
        return Err(PeerParseError::InvalidUrl);
    }
    if authority != "t.me" && authority != "www.t.me" {
        return Err(PeerParseError::UnsupportedUrl);
    }
    let suffix = &after_scheme[authority_end..];
    if suffix.contains('#') {
        return Err(PeerParseError::InvalidUrl);
    }
    let (path, query) = match suffix.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (suffix, None),
    };
    if path.contains('%') || !path.is_ascii() {
        return Err(PeerParseError::InvalidUrl);
    }
    let path = path.strip_prefix('/').ok_or(PeerParseError::InvalidUrl)?;
    let segments: Vec<&str> = path.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(PeerParseError::InvalidUrl);
    }
    if segments.is_empty() {
        return Err(PeerParseError::UnsupportedUrl);
    }

    let (peer, message_id) = match segments.as_slice() {
        [username] => {
            validate_username(username)?;
            if query.is_some() {
                return Err(PeerParseError::InvalidMetadata);
            }
            return Ok(PeerReference::Username {
                username: username.to_ascii_lowercase(),
            });
        }
        [username, message_id] if *username != "c" && *username != "s" => {
            validate_username(username)?;
            (
                MessagePeer::Username {
                    username: username.to_ascii_lowercase(),
                },
                parse_positive_i64(message_id).ok_or(PeerParseError::InvalidMessageId)?,
            )
        }
        ["s", username, message_id] => {
            validate_username(username)?;
            (
                MessagePeer::Username {
                    username: username.to_ascii_lowercase(),
                },
                parse_positive_i64(message_id).ok_or(PeerParseError::InvalidMessageId)?,
            )
        }
        ["c", channel_id, message_id] => (
            MessagePeer::InternalChannelId {
                channel_id: parse_positive_i64(channel_id)
                    .ok_or(PeerParseError::InvalidNumericId)?,
            },
            parse_positive_i64(message_id).ok_or(PeerParseError::InvalidMessageId)?,
        ),
        _ => return Err(PeerParseError::InvalidUrl),
    };

    let metadata = parse_link_metadata(query)?;
    Ok(PeerReference::MessageLink(MessageLink {
        peer,
        message_id,
        metadata,
    }))
}

fn parse_link_metadata(query: Option<&str>) -> Result<MessageLinkMetadata, PeerParseError> {
    let mut metadata = MessageLinkMetadata::default();
    let Some(query) = query else {
        return Ok(metadata);
    };
    if query.is_empty() {
        return Err(PeerParseError::InvalidMetadata);
    }

    let mut seen_thread = false;
    let mut seen_comment = false;
    let mut seen_single = false;
    for pair in query.split('&') {
        if pair == "single" {
            if seen_single {
                return Err(PeerParseError::InvalidMetadata);
            }
            seen_single = true;
            metadata.single = true;
            continue;
        }
        let (key, value) = pair
            .split_once('=')
            .ok_or(PeerParseError::InvalidMetadata)?;
        match key {
            "thread" => {
                if seen_thread {
                    return Err(PeerParseError::InvalidMetadata);
                }
                metadata.thread_id =
                    Some(parse_positive_i64(value).ok_or(PeerParseError::InvalidMetadata)?);
                seen_thread = true;
            }
            "comment" => {
                if seen_comment {
                    return Err(PeerParseError::InvalidMetadata);
                }
                metadata.comment_id =
                    Some(parse_positive_i64(value).ok_or(PeerParseError::InvalidMetadata)?);
                seen_comment = true;
            }
            // Telegram links commonly carry unrelated campaign parameters.
            _ => {}
        }
    }
    Ok(metadata)
}

fn parse_positive_i64(value: &str) -> Option<i64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<i64>().ok().filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usernames_and_normalizes_ascii_case() {
        assert_eq!(
            parse_peer_reference("@Example_123").unwrap(),
            PeerReference::Username {
                username: "example_123".into()
            }
        );
        for bad in [
            "@abc",
            "@_abcdef",
            "@abcdef!",
            "@абвгд",
            "@a.bc",
            "@a_b\u{200b}cdef",
        ] {
            assert!(parse_peer_reference(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_tme_username_and_message_links() {
        assert_eq!(
            parse_peer_reference("https://t.me/Example_123").unwrap(),
            PeerReference::Username {
                username: "example_123".into()
            }
        );
        assert_eq!(
            parse_peer_reference(
                "https://www.t.me/s/Example_123/42?single&thread=8&comment=9&utm_source=chat"
            )
            .unwrap(),
            PeerReference::MessageLink(MessageLink {
                peer: MessagePeer::Username {
                    username: "example_123".into()
                },
                message_id: 42,
                metadata: MessageLinkMetadata {
                    single: true,
                    thread_id: Some(8),
                    comment_id: Some(9)
                }
            })
        );
        assert_eq!(
            parse_peer_reference("https://t.me/c/987654/42").unwrap(),
            PeerReference::MessageLink(MessageLink {
                peer: MessagePeer::InternalChannelId { channel_id: 987654 },
                message_id: 42,
                metadata: MessageLinkMetadata::default()
            })
        );
    }

    #[test]
    fn parses_only_stable_tg_user_numeric_ids() {
        assert_eq!(
            parse_peer_reference("tg://user?id=123456789").unwrap(),
            PeerReference::NumericUserId { user_id: 123456789 }
        );
        for bad in [
            "tg://user?id=0",
            "tg://user?id=-1",
            "tg://user?id=9223372036854775808",
            "tg://resolve?domain=example",
        ] {
            assert!(parse_peer_reference(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_malformed_and_ambiguous_urls() {
        for bad in [
            "https://t.me.evil/username",
            "https://evil.test/t.me/username",
            "https://user@t.me/example",
            "https://t.me:443/example",
            "https://t.me/%65xample",
            "https://t.me/examрle",
            "https://t.me/example/0",
            "https://t.me/example/12?thread=3&thread=4",
            "https://t.me/example/12?single&single",
            "https://t.me/example/12#fragment",
            "http://t.me//example",
        ] {
            assert!(parse_peer_reference(bad).is_err(), "{bad}");
        }
        assert!(parse_peer_reference(&format!("@{}", "a".repeat(2_048))).is_err());
    }

    #[test]
    fn preserves_order_and_trailing_path_is_rejected() {
        let first = parse_peer_reference("https://t.me/user_a/12").unwrap();
        let second = parse_peer_reference("https://t.me/user_b/12").unwrap();
        assert_eq!(
            first,
            parse_peer_reference("https://t.me/user_a/12").unwrap()
        );
        assert_eq!(
            second,
            parse_peer_reference("https://t.me/user_b/12").unwrap()
        );
        assert_ne!(first, second);
        assert!(parse_peer_reference("https://t.me/user_a/12/extra").is_err());
    }

    #[test]
    fn resolution_identity_types_do_not_encode_permissions() {
        let unknown = serde_json::to_value(PeerResolution::Unknown {
            reason: "unsupported by backend".into(),
        })
        .unwrap();
        assert_eq!(unknown["type"], "unknown");
        assert!(unknown.get("can_message").is_none());
    }
}
