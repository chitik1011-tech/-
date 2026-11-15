use grammers_client::{
    Client, SignInError,
    client::{LoginToken, PasswordToken},
};
use tg_mobile_core::{AuthorizationState, CoreError};

use crate::error::{TelegramBackendError, api_error};

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
            .map_err(|error| api_error("check authorization", error))?;
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
        if phone.trim().is_empty() {
            return Err(CoreError::InvalidInput(
                "phone number must not be empty".into(),
            ));
        }

        let token = client
            .request_login_code(phone.trim(), api_hash)
            .await
            .map_err(|error| api_error("request login code", error))?;
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
                Err(api_error("sign in with code", error))
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
                Err(api_error("sign in with 2FA password; restart login", error))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AuthPhase;
    use tg_mobile_core::AuthorizationState;

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
