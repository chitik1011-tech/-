use grammers_client::{
    Client, SignInError,
    client::{LoginToken, PasswordToken},
    sender::InvocationError,
    tl::enums::auth::SentCodeType,
};
use std::{future::Future, time::Duration};
use tg_mobile_core::{AuthorizationState, CoreError, LoginCodeDeliveryMethod};
use tokio::time::timeout;

use crate::error::{TelegramBackendError, auth_api_error};

const AUTH_RPC_TIMEOUT: Duration = Duration::from_secs(30);

async fn wait_for_auth_rpc<T, E>(
    future: impl Future<Output = std::result::Result<T, E>>,
    timeout_error: CoreError,
) -> tg_mobile_core::Result<std::result::Result<T, E>> {
    timeout(AUTH_RPC_TIMEOUT, future)
        .await
        .map_err(|_| timeout_error)
}

fn auth_timeout_error(operation: &'static str, outcome_unknown: bool) -> CoreError {
    if outcome_unknown {
        TelegramBackendError::Authorization(format!(
            "{operation} timed out; Telegram may have accepted the request, so restart or verify authorization before retrying"
        ))
        .into()
    } else {
        CoreError::TransientTransport(format!(
            "{operation} timed out; no result was received, retry only after checking whether Telegram completed it"
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthPhase {
    Closed,
    WaitingForPhoneNumber,
    WaitingForCode,
    WaitingForPassword,
    Ready,
}

fn delivery_method_from_sent_code_type(delivery: SentCodeType) -> LoginCodeDeliveryMethod {
    match delivery {
        SentCodeType::App(_) => LoginCodeDeliveryMethod::TelegramApp,
        SentCodeType::Sms(_) => LoginCodeDeliveryMethod::Sms,
        SentCodeType::Call(_) => LoginCodeDeliveryMethod::VoiceCall,
        SentCodeType::FlashCall(_) => LoginCodeDeliveryMethod::FlashCall,
        SentCodeType::MissedCall(_) => LoginCodeDeliveryMethod::MissedCall,
        SentCodeType::FirebaseSms(_) => LoginCodeDeliveryMethod::FirebaseSms,
        SentCodeType::FragmentSms(_) => LoginCodeDeliveryMethod::FragmentSms,
        SentCodeType::SmsWord(_) => LoginCodeDeliveryMethod::SmsWord,
        SentCodeType::SmsPhrase(_) => LoginCodeDeliveryMethod::SmsPhrase,
        SentCodeType::EmailCode(_) => LoginCodeDeliveryMethod::EmailCode,
        SentCodeType::SetUpEmailRequired(_) => LoginCodeDeliveryMethod::EmailSetupRequired,
    }
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
    login_code_delivery: Option<LoginCodeDeliveryMethod>,
}

impl Default for AuthFlow {
    fn default() -> Self {
        Self {
            phase: AuthPhase::Closed,
            login_token: None,
            password_token: None,
            login_code_delivery: None,
        }
    }
}

impl AuthFlow {
    pub(crate) fn state(&self) -> AuthorizationState {
        self.phase.core_state()
    }

    pub(crate) fn login_code_delivery(&self) -> Option<LoginCodeDeliveryMethod> {
        self.login_code_delivery
    }

    pub(crate) fn close(&mut self) {
        self.login_token = None;
        self.password_token = None;
        self.login_code_delivery = None;
        self.phase = AuthPhase::Closed;
    }

    fn reset_to_phone_entry(&mut self) {
        self.login_token = None;
        self.password_token = None;
        self.login_code_delivery = None;
        self.phase = AuthPhase::WaitingForPhoneNumber;
    }

    pub(crate) async fn start(
        &mut self,
        client: &Client,
    ) -> tg_mobile_core::Result<AuthorizationState> {
        let authorized = client
            .is_authorized()
            .await
            .map_err(|error| auth_api_error("check authorization", error))?;
        if authorized {
            self.login_token = None;
            self.password_token = None;
            self.login_code_delivery = None;
            self.phase = AuthPhase::Ready;
        } else {
            self.reset_to_phone_entry();
        }
        Ok(self.state())
    }

    pub(crate) async fn set_phone_number(
        &mut self,
        client: &Client,
        api_hash: &str,
        phone: String,
    ) -> tg_mobile_core::Result<AuthorizationState> {
        // A new phone request replaces the previous token/metadata even if
        // input validation or the Telegram RPC fails.
        self.reset_to_phone_entry();
        let phone = normalize_phone_number(&phone)?;

        let (token, delivery) = wait_for_auth_rpc(
            client.request_login_code_with_delivery_type(&phone, api_hash),
            auth_timeout_error("request login code", true),
        )
        .await?
        .map_err(|error| auth_api_error("request login code", error))?;
        self.password_token = None;
        self.login_token = Some(token);
        self.login_code_delivery = Some(delivery_method_from_sent_code_type(delivery));
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

        let result = match wait_for_auth_rpc(
            client.sign_in(&token, code.trim()),
            auth_timeout_error("sign in with code", true),
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                self.reset_to_phone_entry();
                return Err(error);
            }
        };

        match result {
            Ok(_) => {
                self.password_token = None;
                self.login_code_delivery = None;
                self.phase = AuthPhase::Ready;
                Ok(self.state())
            }
            Err(SignInError::PasswordRequired(password_token)) => {
                self.password_token = Some(password_token);
                self.login_code_delivery = None;
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
                self.reset_to_phone_entry();
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
                if login_token_invalidated(&error) {
                    self.reset_to_phone_entry();
                } else {
                    self.login_token = Some(token);
                    self.phase = AuthPhase::WaitingForCode;
                }
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

        let result = match wait_for_auth_rpc(
            client.check_password(token, password.as_bytes()),
            auth_timeout_error("sign in with 2FA password", true),
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                self.reset_to_phone_entry();
                return Err(error);
            }
        };

        match result {
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
            Err(SignInError::InvalidCode) => {
                self.reset_to_phone_entry();
                Err(TelegramBackendError::Authorization(
                    "Telegram returned an unexpected invalid-code error during 2FA".into(),
                )
                .into())
            }
            Err(SignInError::SignUpRequired) => {
                self.reset_to_phone_entry();
                Err(TelegramBackendError::Authorization(
                    "account sign-up must be completed in an official Telegram client".into(),
                )
                .into())
            }
            Err(SignInError::Other(error)) => {
                // `check_password` consumes the password token. Generic transport/RPC
                // failures do not return a replacement token, so restart the login
                // flow rather than pretending a retry can succeed with stale state.
                self.reset_to_phone_entry();
                Err(auth_api_error(
                    "sign in with 2FA password; restart login",
                    error,
                ))
            }
        }
    }

    pub(crate) async fn logout(client: &Client) -> tg_mobile_core::Result<()> {
        let authorized = wait_for_auth_rpc(
            client.is_authorized(),
            auth_timeout_error("check authorization before logout", false),
        )
        .await?
        .map_err(|error| auth_api_error("check authorization before logout", error))?;
        if authorized {
            wait_for_auth_rpc(
                client.sign_out(),
                auth_timeout_error("log out of Telegram", true),
            )
            .await?
            .map_err(|error| auth_api_error("log out of Telegram", error))?;
        }
        Ok(())
    }
}

/// A code cannot be retried with its LoginToken after these RPC responses.
/// Grammers reports ordinary wrong codes as `SignInError::InvalidCode`, which
/// remains retryable and is handled separately above.
fn login_token_invalidated(error: &InvocationError) -> bool {
    matches!(
        error,
        InvocationError::Rpc(rpc)
            if matches!(
                rpc.name.as_str(),
                "PHONE_CODE_EXPIRED"
                    | "PHONE_CODE_HASH_EMPTY"
                    | "PHONE_CODE_HASH_INVALID"
                    | "AUTH_RESTART"
            )
    )
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
    use std::future::pending;

    use super::{
        AUTH_RPC_TIMEOUT, AuthFlow, AuthPhase, auth_timeout_error,
        delivery_method_from_sent_code_type, login_token_invalidated, normalize_phone_number,
        wait_for_auth_rpc,
    };
    use grammers_client::sender::{InvocationError, RpcError};
    use grammers_client::tl;
    use tg_mobile_core::{AuthorizationState, CoreError, LoginCodeDeliveryMethod};

    #[tokio::test(start_paused = true)]
    async fn auth_rpc_deadline_returns_ambiguous_operation_error() {
        let task = tokio::spawn(wait_for_auth_rpc::<(), ()>(
            pending(),
            auth_timeout_error("sign in with code", true),
        ));
        tokio::task::yield_now().await;
        tokio::time::advance(AUTH_RPC_TIMEOUT).await;

        assert!(matches!(
            task.await.unwrap(),
            Err(CoreError::Backend(message))
                if message.contains("sign in with code timed out")
                    && message.contains("may have accepted")
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn authorization_probe_timeout_is_retryable_without_claiming_logout() {
        let task = tokio::spawn(wait_for_auth_rpc::<(), ()>(
            pending(),
            auth_timeout_error("check authorization before logout", false),
        ));
        tokio::task::yield_now().await;
        tokio::time::advance(AUTH_RPC_TIMEOUT).await;

        assert!(matches!(
            task.await.unwrap(),
            Err(CoreError::TransientTransport(message))
                if message.contains("check authorization before logout timed out")
        ));
    }

    #[tokio::test]
    async fn completed_auth_rpc_preserves_success_and_server_error() {
        assert!(matches!(
            wait_for_auth_rpc(
                async { Ok::<_, ()>(true) },
                CoreError::Backend("timeout".into())
            )
            .await,
            Ok(Ok(true))
        ));
        assert!(matches!(
            wait_for_auth_rpc(
                async { Err::<(), _>(7) },
                CoreError::Backend("timeout".into())
            )
            .await,
            Ok(Err(7))
        ));
    }

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

    #[test]
    fn preserves_exact_telegram_code_delivery_type() {
        let app =
            tl::enums::auth::SentCodeType::App(tl::types::auth::SentCodeTypeApp { length: 5 });
        let sms =
            tl::enums::auth::SentCodeType::Sms(tl::types::auth::SentCodeTypeSms { length: 5 });

        assert_eq!(
            delivery_method_from_sent_code_type(app),
            LoginCodeDeliveryMethod::TelegramApp
        );
        assert_eq!(
            delivery_method_from_sent_code_type(sms),
            LoginCodeDeliveryMethod::Sms
        );

        let mut flow = AuthFlow::default();
        assert_eq!(flow.login_code_delivery(), None);
        flow.login_code_delivery = Some(LoginCodeDeliveryMethod::TelegramApp);
        assert_eq!(
            flow.login_code_delivery(),
            Some(LoginCodeDeliveryMethod::TelegramApp)
        );
        flow.reset_to_phone_entry();
        assert_eq!(flow.login_code_delivery(), None);
    }

    #[test]
    fn maps_every_grammers_sent_code_type_exhaustively() {
        use tl::types::auth as sent_code;

        let cases = [
            (
                tl::enums::auth::SentCodeType::App(sent_code::SentCodeTypeApp { length: 5 }),
                LoginCodeDeliveryMethod::TelegramApp,
            ),
            (
                tl::enums::auth::SentCodeType::Sms(sent_code::SentCodeTypeSms { length: 5 }),
                LoginCodeDeliveryMethod::Sms,
            ),
            (
                tl::enums::auth::SentCodeType::Call(sent_code::SentCodeTypeCall { length: 5 }),
                LoginCodeDeliveryMethod::VoiceCall,
            ),
            (
                tl::enums::auth::SentCodeType::FlashCall(sent_code::SentCodeTypeFlashCall {
                    pattern: "***".into(),
                }),
                LoginCodeDeliveryMethod::FlashCall,
            ),
            (
                tl::enums::auth::SentCodeType::MissedCall(sent_code::SentCodeTypeMissedCall {
                    prefix: "+1".into(),
                    length: 5,
                }),
                LoginCodeDeliveryMethod::MissedCall,
            ),
            (
                tl::enums::auth::SentCodeType::FirebaseSms(sent_code::SentCodeTypeFirebaseSms {
                    nonce: None,
                    play_integrity_project_id: None,
                    play_integrity_nonce: None,
                    receipt: None,
                    push_timeout: None,
                    length: 5,
                }),
                LoginCodeDeliveryMethod::FirebaseSms,
            ),
            (
                tl::enums::auth::SentCodeType::FragmentSms(sent_code::SentCodeTypeFragmentSms {
                    url: "https://fragment.com".into(),
                    length: 5,
                }),
                LoginCodeDeliveryMethod::FragmentSms,
            ),
            (
                tl::enums::auth::SentCodeType::SmsWord(sent_code::SentCodeTypeSmsWord {
                    beginning: Some("A".into()),
                }),
                LoginCodeDeliveryMethod::SmsWord,
            ),
            (
                tl::enums::auth::SentCodeType::SmsPhrase(sent_code::SentCodeTypeSmsPhrase {
                    beginning: Some("A".into()),
                }),
                LoginCodeDeliveryMethod::SmsPhrase,
            ),
            (
                tl::enums::auth::SentCodeType::EmailCode(sent_code::SentCodeTypeEmailCode {
                    apple_signin_allowed: true,
                    google_signin_allowed: true,
                    email_pattern: "a***@example.com".into(),
                    length: 5,
                    reset_available_period: None,
                    reset_pending_date: None,
                }),
                LoginCodeDeliveryMethod::EmailCode,
            ),
            (
                tl::enums::auth::SentCodeType::SetUpEmailRequired(
                    sent_code::SentCodeTypeSetUpEmailRequired {
                        apple_signin_allowed: true,
                        google_signin_allowed: true,
                    },
                ),
                LoginCodeDeliveryMethod::EmailSetupRequired,
            ),
        ];

        for (sent_code_type, expected) in cases {
            assert_eq!(
                delivery_method_from_sent_code_type(sent_code_type),
                expected
            );
        }
    }

    #[test]
    fn invalid_or_expired_login_hashes_require_a_fresh_phone_entry() {
        for name in [
            "PHONE_CODE_EXPIRED",
            "PHONE_CODE_HASH_EMPTY",
            "PHONE_CODE_HASH_INVALID",
            "AUTH_RESTART",
        ] {
            assert!(
                login_token_invalidated(&InvocationError::Rpc(RpcError {
                    code: 400,
                    name: name.into(),
                    value: None,
                    caused_by: None,
                })),
                "{name} should invalidate its LoginToken"
            );
        }

        for retryable_name in ["PHONE_CODE_INVALID", "FLOOD_WAIT_30"] {
            assert!(
                !login_token_invalidated(&InvocationError::Rpc(RpcError {
                    code: 400,
                    name: retryable_name.into(),
                    value: None,
                    caused_by: None,
                })),
                "{retryable_name} should not invalidate its LoginToken"
            );
        }
    }

    #[test]
    fn consumed_password_token_recovery_does_not_leave_password_stage_active() {
        let mut flow = AuthFlow::default();
        flow.phase = AuthPhase::WaitingForPassword;

        flow.reset_to_phone_entry();

        assert_eq!(flow.state(), AuthorizationState::WaitingForPhoneNumber);
        assert!(flow.login_token.is_none());
        assert!(flow.password_token.is_none());
    }
}
