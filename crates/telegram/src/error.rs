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
        InvocationError::Rpc(error) if error.name == "FLOOD_WAIT" => CoreError::FloodWait {
            seconds: error.value.unwrap_or_default(),
        },
        InvocationError::Rpc(error) if error.code >= 500 => {
            CoreError::TransientSend(format!("{context}: Telegram server error"))
        }
        _ => TelegramBackendError::Authorization(format!("{context}: Telegram request failed"))
            .into(),
    }
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
        assert!(matches!(temporary, CoreError::TransientSend(_)));
    }
}
