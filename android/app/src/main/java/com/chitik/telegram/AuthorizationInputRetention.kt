package com.chitik.telegram

/**
 * Identifies temporary values entered during Telegram authorization.
 *
 * These values are held only in Compose memory and should be cleared when Rust
 * confirms that the authorization flow has moved past the corresponding input step.
 */
internal enum class AuthorizationInputField {
    ApiId,
    ApiHash,
    PhoneNumber,
    LoginCode,
    TwoFactorPassword,
}

/** Returns the sensitive form inputs that no longer match the caller-owned state. */
internal fun authorizationInputsToClear(
    state: AuthorizationUiState,
): Set<AuthorizationInputField> = buildSet {
    if (state.isTelegramConfigured) {
        add(AuthorizationInputField.ApiId)
        add(AuthorizationInputField.ApiHash)
    }

    if (state.authorization != AuthorizationUiStep.WaitingForPhoneNumber) {
        add(AuthorizationInputField.PhoneNumber)
    }
    if (state.authorization != AuthorizationUiStep.WaitingForCode) {
        add(AuthorizationInputField.LoginCode)
    }
    if (state.authorization != AuthorizationUiStep.WaitingForPassword) {
        add(AuthorizationInputField.TwoFactorPassword)
    }
}
