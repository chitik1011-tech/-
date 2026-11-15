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

pub(crate) fn auth_api_error(
    context: &'static str,
    _error: impl std::fmt::Display,
) -> CoreError {
    TelegramBackendError::Authorization(format!("{context}: Telegram request failed")).into()
}

pub(crate) fn session_error(context: &'static str, error: impl std::fmt::Display) -> CoreError {
    TelegramBackendError::Session(format!("{context}: {error}")).into()
}

#[cfg(test)]
mod tests {
    use super::TelegramBackendError;
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
            "PHONE=+15551234567 CODE=12345 PASSWORD=secret",
        );

        match error {
            CoreError::Backend(message) => {
                assert!(message.contains("sign in with code"));
                assert!(!message.contains("+15551234567"));
                assert!(!message.contains("12345"));
                assert!(!message.contains("secret"));
            }
            other => panic!("expected backend error, got {other:?}"),
        }
    }
}
