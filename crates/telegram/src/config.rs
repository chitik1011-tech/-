use std::{fmt, path::PathBuf};

use crate::error::TelegramBackendError;

/// Runtime configuration supplied by the Android/application layer.
///
/// `api_hash` is deliberately redacted from `Debug`. The session database path
/// should point to app-private storage on Android.
#[derive(Clone)]
pub struct TelegramConfig {
    pub api_id: i32,
    pub api_hash: String,
    pub session_path: PathBuf,
    pub catch_up_updates: bool,
    pub update_channel_capacity: usize,
}

impl TelegramConfig {
    pub fn new(api_id: i32, api_hash: impl Into<String>, session_path: impl Into<PathBuf>) -> Self {
        Self {
            api_id,
            api_hash: api_hash.into(),
            session_path: session_path.into(),
            catch_up_updates: true,
            update_channel_capacity: 512,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), TelegramBackendError> {
        if self.api_id <= 0 {
            return Err(TelegramBackendError::Configuration(
                "api_id must be a positive integer".into(),
            ));
        }
        if self.api_hash.trim().is_empty() {
            return Err(TelegramBackendError::Configuration(
                "api_hash must not be empty".into(),
            ));
        }
        if self.session_path.as_os_str().is_empty() {
            return Err(TelegramBackendError::Configuration(
                "session_path must not be empty".into(),
            ));
        }
        if self.update_channel_capacity == 0 {
            return Err(TelegramBackendError::Configuration(
                "update_channel_capacity must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramConfig")
            .field("api_id", &self.api_id)
            .field("api_hash", &"<redacted>")
            .field("session_path", &self.session_path)
            .field("catch_up_updates", &self.catch_up_updates)
            .field("update_channel_capacity", &self.update_channel_capacity)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::TelegramConfig;

    #[test]
    fn debug_redacts_api_hash() {
        let config = TelegramConfig::new(1, "very-secret-hash", "telegram.session");
        let output = format!("{config:?}");
        assert!(!output.contains("very-secret-hash"));
        assert!(output.contains("<redacted>"));
    }

    #[test]
    fn invalid_configuration_is_rejected() {
        let config = TelegramConfig::new(0, "", "");
        assert!(config.validate().is_err());
    }
}
