use grammers_client::sender::InvocationError;
use tg_mobile_core::CoreError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TelegramBackendError {
    #[error("invalid Telegram configuration: {0}")]
    Configuration(String),

    #[error("invalid Telegram input: {0}")]
    InvalidInput(String),

    #[error("Telegram authorization error: {0}")]
    Authorization(String),

    #[error("Telegram API error: {0}")]
    Api(String),

    #[error("Telegram session error: {0}")]
    Session(String),

    #[error("Telegram initial connection timed out while checking authorization")]
    InitialConnectionTimeout,

    #[error("chat {0} is not available in the peer cache; load chats first")]
    UnknownChat(i64),

    #[error("Telegram update stream is no longer available")]
    UpdateStreamUnavailable,

    #[error("Telegram backend has been stopped")]
    Stopped,
}

impl From<TelegramBackendError> for CoreError {
    fn from(value: TelegramBackendError) -> Self {
        match value {
            TelegramBackendError::InitialConnectionTimeout => CoreError::TransientTransport(
                "Telegram initial connection timed out while checking authorization".into(),
            ),
            TelegramBackendError::Configuration(message)
            | TelegramBackendError::InvalidInput(message) => CoreError::InvalidInput(message),
            TelegramBackendError::UnknownChat(chat_id) => CoreError::InvalidInput(format!(
                "chat {chat_id} is not available in the Telegram peer cache; call LoadChats first"
            )),
            other => CoreError::Backend(other.to_string()),
        }
    }
}

pub(crate) fn api_error(context: &'static str, error: impl std::fmt::Display) -> CoreError {
    TelegramBackendError::Api(format!("{context}: {error}")).into()
}

pub(crate) fn auth_api_error(context: &'static str, error: InvocationError) -> CoreError {
    match error {
        InvocationError::Io(_) | InvocationError::Transport(_) => {
            CoreError::TransientTransport(format!("{context}: Telegram transport unavailable"))
        }
        InvocationError::Rpc(error) => auth_rpc_error(context, error),
        _ => TelegramBackendError::Authorization(format!("{context}: Telegram request failed"))
            .into(),
    }
}

fn auth_rpc_error(context: &'static str, error: grammers_client::sender::RpcError) -> CoreError {
    let rpc_name = safe_rpc_name(&error.name);
    if let Some(seconds) = flood_wait_seconds(&error.name, error.value) {
        return CoreError::FloodWait { seconds };
    }
    if error.code >= 500 {
        return CoreError::TransientSend(format!(
            "{context}: Telegram RPC {rpc_name} (code {})",
            error.code
        ));
    }

    // Keep the server's actual RPC class visible for diagnosis, but constrain
    // it to Telegram's identifier alphabet so arbitrary error text or
    // request parameters cannot reach the UI.
    let detail = match error.name.as_str() {
        "PHONE_NUMBER_INVALID" => "PHONE_NUMBER_INVALID: enter a valid international phone number",
        "PHONE_NUMBER_BANNED" => "PHONE_NUMBER_BANNED: Telegram has banned this number",
        "API_ID_INVALID" => "API_ID_INVALID: check the Telegram API ID and API hash",
        "API_ID_PUBLISHED_FLOOD" => "API_ID_PUBLISHED_FLOOD: Telegram has restricted this API ID",
        "PHONE_CODE_INVALID" => "PHONE_CODE_INVALID: incorrect login code",
        "PHONE_CODE_EXPIRED" => "PHONE_CODE_EXPIRED: request a new login code",
        "PHONE_CODE_EMPTY" => "PHONE_CODE_EMPTY: enter the login code",
        "PHONE_CODE_HASH_EMPTY" => "PHONE_CODE_HASH_EMPTY: request a new login code",
        "PHONE_CODE_HASH_INVALID" => "PHONE_CODE_HASH_INVALID: request a new login code",
        "SESSION_PASSWORD_NEEDED" => "SESSION_PASSWORD_NEEDED: two-step verification is required",
        "PASSWORD_HASH_INVALID" => {
            "PASSWORD_HASH_INVALID: incorrect two-step verification password"
        }
        "PHONE_NUMBER_FLOOD" => "PHONE_NUMBER_FLOOD: too many attempts for this number",
        "AUTH_RESTART" => "AUTH_RESTART: restart Telegram login",
        _ => "Telegram rejected the authorization request",
    };
    TelegramBackendError::Authorization(format!(
        "{context}: {detail} (RPC {rpc_name}, code {})",
        error.code
    ))
    .into()
}

fn safe_rpc_name(name: &str) -> String {
    let bytes = name.as_bytes();
    let valid = bytes.len() <= 80
        && bytes.first().is_some_and(|byte| byte.is_ascii_uppercase())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_');
    if valid { name.to_owned() } else { "UNKNOWN".into() }
}

/// Grammers can expose Telegram flood limits either as `FLOOD_WAIT` plus a
/// numeric RPC value or as the wire-style `FLOOD_WAIT_<seconds>` name. Keep
/// both forms actionable without reflecting arbitrary RPC names to the UI.
fn flood_wait_seconds(name: &str, value: Option<u32>) -> Option<u32> {
    if name == "FLOOD_WAIT" {
        return value.filter(|seconds| *seconds > 0);
    }

    name.strip_prefix("FLOOD_WAIT_")
        .and_then(|seconds| seconds.parse::<u32>().ok())
        .filter(|seconds| *seconds > 0)
}

pub(crate) fn session_error(context: &'static str, error: impl std::fmt::Display) -> CoreError {
    TelegramBackendError::Session(format!("{context}: {error}")).into()
}

#[cfg(test)]
mod tests {
    use super::TelegramBackendError;
    use grammers_client::sender::{InvocationError, RpcError};
    use tg_mobile_core::CoreError;

    #[test]
    fn bad_user_input_maps_to_core_invalid_input() {
        let error: CoreError = TelegramBackendError::InvalidInput("bad code".into()).into();
        assert!(matches!(error, CoreError::InvalidInput(message) if message == "bad code"));
    }

    #[test]
    fn api_failure_maps_to_core_backend_error() {
        let error: CoreError = TelegramBackendError::Api("network down".into()).into();
        assert!(matches!(error, CoreError::Backend(message) if message.contains("network down")));
    }

    #[test]
    fn authorization_api_errors_do_not_expose_upstream_details() {
        let error = super::auth_api_error(
            "sign in with code",
            InvocationError::Io(std::io::Error::other(
                "PHONE=+15551234567 CODE=12345 PASSWORD=secret",
            )),
        );

        match error {
            CoreError::TransientTransport(message) => {
                assert!(message.contains("sign in with code"));
                assert!(!message.contains("+15551234567"));
                assert!(!message.contains("12345"));
                assert!(!message.contains("secret"));
            }
            other => panic!("expected backend error, got {other:?}"),
        }
    }

    #[test]
    fn auth_rpc_errors_preserve_real_rpc_class_without_reflecting_arbitrary_text() {
        for (name, expected) in [
            ("PHONE_NUMBER_INVALID", "PHONE_NUMBER_INVALID"),
            ("PHONE_NUMBER_BANNED", "PHONE_NUMBER_BANNED"),
            ("API_ID_INVALID", "API_ID_INVALID"),
            ("API_ID_PUBLISHED_FLOOD", "API_ID_PUBLISHED_FLOOD"),
            ("PHONE_CODE_INVALID", "PHONE_CODE_INVALID"),
            ("PHONE_CODE_EXPIRED", "PHONE_CODE_EXPIRED"),
            ("SESSION_PASSWORD_NEEDED", "SESSION_PASSWORD_NEEDED"),
        ] {
            let error = super::auth_api_error(
                "request login code",
                InvocationError::Rpc(RpcError {
                    code: 400,
                    name: name.into(),
                    value: None,
                    caused_by: None,
                }),
            );
            assert!(matches!(error, CoreError::Backend(message) if message.contains(expected)));
        }

        let valid_unknown = super::auth_api_error(
            "request login code",
            InvocationError::Rpc(RpcError {
                code: 400,
                name: "CUSTOM_RPC_42".into(),
                value: None,
                caused_by: None,
            }),
        );
        assert!(matches!(valid_unknown, CoreError::Backend(message)
            if message.contains("RPC CUSTOM_RPC_42, code 400")));

        let parameter_like = super::auth_api_error(
            "request login code",
            InvocationError::Rpc(RpcError {
                code: 400,
                name: "SECRET=API_HASH".into(),
                value: None,
                caused_by: None,
            }),
        );
        assert!(matches!(parameter_like, CoreError::Backend(message)
            if message.contains("RPC UNKNOWN, code 400")
                && !message.contains("SECRET")
                && !message.contains("API_HASH")));
    }

    #[test]
    fn auth_flood_wait_and_temporary_rpc_errors_keep_distinct_classes() {
        let flood = super::auth_api_error(
            "check authorization",
            InvocationError::Rpc(RpcError {
                code: 420,
                name: "FLOOD_WAIT".into(),
                value: Some(12),
                caused_by: None,
            }),
        );
        assert!(matches!(flood, CoreError::FloodWait { seconds: 12 }));

        let temporary = super::auth_api_error(
            "check authorization",
            InvocationError::Rpc(RpcError {
                code: 500,
                name: "INTERNAL".into(),
                value: None,
                caused_by: None,
            }),
        );
        assert!(matches!(temporary, CoreError::TransientSend(message)
            if message.contains("RPC INTERNAL (code 500")));
    }

    #[test]
    fn auth_flood_wait_accepts_grammers_wire_suffix_form() {
        let error = super::auth_api_error(
            "request login code",
            InvocationError::Rpc(RpcError {
                code: 420,
                name: "FLOOD_WAIT_47".into(),
                value: None,
                caused_by: None,
            }),
        );
        assert!(matches!(error, CoreError::FloodWait { seconds: 47 }));

        let malformed = super::auth_api_error(
            "request login code",
            InvocationError::Rpc(RpcError {
                code: 420,
                name: "FLOOD_WAIT_invalid".into(),
                value: None,
                caused_by: None,
            }),
        );
        assert!(matches!(malformed, CoreError::Backend(message) if !message.contains("invalid")));
    }
}
