use grammers_client::{
    Client, SignInError,
    client::{LoginToken, PasswordToken},
};
use tg_mobile_core::{AuthorizationState, CoreError};

use crate::error::{TelegramBackendError, auth_api_error};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthPhase {
    Closed,
    WaitingForPhoneNumber,
    WaitingForCode,
    WaitingForPassword,
    Ready,
}

impl AuthPhase {
    fn core_state(self) -> AuthorizationState {
        match self {
            Self::Closed => AuthorizationState::Closed,
            Self::WaitingForPhoneNumber => AuthorizationState::WaitingForPhoneNumber,
            Self::WaitingForCode => AuthorizationState::WaitingForCode,
            Self::WaitingForPassword => AuthorizationState::WaitingForPassword,
            Self::Ready => AuthorizationState::Ready,
        }
    }
}

pub(crate) struct AuthFlow {
    phase: AuthPhase,
    login_token: Option<LoginToken>,
    password_token: Option<PasswordToken>,
}

impl Default for AuthFlow {
    fn default() -> Self {
        Self {
            phase: AuthPhase::Closed,
            login_token: None,
            password_token: None,
        }
    }
}

impl AuthFlow {
    pub(crate) fn state(&self) -> AuthorizationState {
        self.phase.core_state()
    }

    pub(crate) fn close(&mut self) {
        self.login_token = None;
        self.password_token = None;
        self.phase = AuthPhase::Closed;
    }

    pub(crate) async fn start(
        &mut self,
        client: &Client,
    ) -> tg_mobile_core::Result<AuthorizationState> {
        let authorized = client
            .is_authorized()
            .await
            .map_err(|error| auth_api_error("check authorization", error))?;
        self.phase = if authorized {
            AuthPhase::Ready
        } else {
            AuthPhase::WaitingForPhoneNumber
        };
        Ok(self.state())
    }

    pub(crate) async fn set_phone_number(
        &mut self,
        client: &Client,
        api_hash: &str,
        phone: String,
    ) -> tg_mobile_core::Result<AuthorizationState> {
        let phone = normalize_phone_number(&phone)?;

        let token = client
            .request_login_code(&phone, api_hash)
            .await
            .map_err(|error| auth_api_error("request login code", error))?;
        self.password_token = None;
        self.login_token = Some(token);
        self.phase = AuthPhase::WaitingForCode;
        Ok(self.state())
    }

    pub(crate) async fn submit_code(
        &mut self,
        client: &Client,
        code: String,
    ) -> tg_mobile_core::Result<AuthorizationState> {
        if code.trim().is_empty() {
            return Err(CoreError::InvalidInput(
                "login code must not be empty".into(),
            ));
        }

        let token = self.login_token.take().ok_or_else(|| {
            CoreError::InvalidInput("request a login code before submitting one".into())
        })?;

        match client.sign_in(&token, code.trim()).await {
            Ok(_) => {
                self.password_token = None;
                self.phase = AuthPhase::Ready;
                Ok(self.state())
            }
            Err(SignInError::PasswordRequired(password_token)) => {
                self.password_token = Some(password_token);
                self.phase = AuthPhase::WaitingForPassword;
                Ok(self.state())
            }
            Err(SignInError::InvalidCode) => {
                self.login_token = Some(token);
                self.phase = AuthPhase::WaitingForCode;
                Err(CoreError::InvalidInput(
                    "invalid Telegram login code".into(),
                ))
            }
            Err(SignInError::SignUpRequired) => {
                self.phase = AuthPhase::WaitingForPhoneNumber;
                Err(TelegramBackendError::Authorization(
                    "account sign-up must be completed in an official Telegram client".into(),
                )
                .into())
            }
            Err(SignInError::InvalidPassword(password_token)) => {
                self.password_token = Some(password_token);
                self.phase = AuthPhase::WaitingForPassword;
                Err(CoreError::InvalidInput(
                    "invalid Telegram 2FA password".into(),
                ))
            }
            Err(SignInError::Other(error)) => {
                self.login_token = Some(token);
                self.phase = AuthPhase::WaitingForCode;
                Err(auth_api_error("sign in with code", error))
            }
        }
    }

    pub(crate) async fn submit_password(
        &mut self,
        client: &Client,
        password: String,
    ) -> tg_mobile_core::Result<AuthorizationState> {
        if password.is_empty() {
            return Err(CoreError::InvalidInput(
                "2FA password must not be empty".into(),
            ));
        }

        let token = self.password_token.take().ok_or_else(|| {
            CoreError::InvalidInput("Telegram did not request a 2FA password".into())
        })?;

        match client.check_password(token, password.as_bytes()).await {
            Ok(_) => {
                self.phase = AuthPhase::Ready;
                Ok(self.state())
            }
            Err(SignInError::InvalidPassword(password_token)) => {
                self.password_token = Some(password_token);
                self.phase = AuthPhase::WaitingForPassword;
                Err(CoreError::InvalidInput(
                    "invalid Telegram 2FA password".into(),
                ))
            }
            Err(SignInError::PasswordRequired(password_token)) => {
                self.password_token = Some(password_token);
                self.phase = AuthPhase::WaitingForPassword;
                Err(CoreError::InvalidInput(
                    "Telegram still requires a 2FA password".into(),
                ))
            }
            Err(SignInError::InvalidCode) => Err(TelegramBackendError::Authorization(
                "Telegram returned an unexpected invalid-code error during 2FA".into(),
            )
            .into()),
            Err(SignInError::SignUpRequired) => Err(TelegramBackendError::Authorization(
                "account sign-up must be completed in an official Telegram client".into(),
            )
            .into()),
            Err(SignInError::Other(error)) => {
                // `check_password` consumes the password token. Generic transport/RPC
                // failures do not return a replacement token, so restart the login
                // flow rather than pretending a retry can succeed with stale state.
                self.phase = AuthPhase::WaitingForPhoneNumber;
                Err(auth_api_error(
                    "sign in with 2FA password; restart login",
                    error,
                ))
            }
        }
    }
}

/// Accept country-code numbers and common visual separators, but never guess a
/// country prefix. Telegram expects an international phone number.
fn normalize_phone_number(input: &str) -> tg_mobile_core::Result<String> {
    let stripped: String = input
        .chars()
        .filter(|c| !matches!(*c, ' ' | '-' | '(' | ')'))
        .collect();
    let digits = stripped.strip_prefix('+').unwrap_or(&stripped);
    if !(8..=15).contains(&digits.len())
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || digits.starts_with('0')
    {
        return Err(CoreError::InvalidInput(
            "PHONE_NUMBER_INVALID: enter a number with country code, e.g. +15551234567".into(),
        ));
    }
    Ok(format!("+{digits}"))
}

#[cfg(test)]
mod tests {
    use super::{AuthPhase, normalize_phone_number};
    use tg_mobile_core::AuthorizationState;

    #[test]
    fn international_phone_numbers_are_normalized_and_invalid_numbers_rejected() {
        assert_eq!(
            normalize_phone_number("+1 (555) 123-4567").unwrap(),
            "+15551234567"
        );
        assert_eq!(
            normalize_phone_number("15551234567").unwrap(),
            "+15551234567"
        );
        for invalid in [
            "",
            "123",
            "+0123456789",
            "1234567890123456",
            "+1555abc4567",
            "++15551234567",
        ] {
            assert!(normalize_phone_number(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn maps_authentication_phases_to_core_states() {
        assert_eq!(AuthPhase::Closed.core_state(), AuthorizationState::Closed);
        assert_eq!(
            AuthPhase::WaitingForPhoneNumber.core_state(),
            AuthorizationState::WaitingForPhoneNumber
        );
        assert_eq!(
            AuthPhase::WaitingForCode.core_state(),
            AuthorizationState::WaitingForCode
        );
        assert_eq!(
            AuthPhase::WaitingForPassword.core_state(),
            AuthorizationState::WaitingForPassword
        );
        assert_eq!(AuthPhase::Ready.core_state(), AuthorizationState::Ready);
    }
}
