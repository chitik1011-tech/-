package com.chitik.telegram

import org.json.JSONObject

/** Actual code delivery type returned by Telegram's auth.sendCode response. */
enum class LoginCodeDeliveryMethod {
    TELEGRAM_APP,
    SMS,
    VOICE_CALL,
    FLASH_CALL,
    MISSED_CALL,
    FIREBASE_SMS,
    FRAGMENT_SMS,
    SMS_WORD,
    SMS_PHRASE,
    EMAIL_CODE,
    EMAIL_SETUP_REQUIRED,
}

/** Unknown values are intentionally treated as absent rather than guessed. */
internal fun parseLoginCodeDeliveryEvent(payload: String): LoginCodeDeliveryMethod? = runCatching {
    val value = JSONObject(payload).opt("payload") as? String ?: return@runCatching null
    when (value) {
        "telegram_app" -> LoginCodeDeliveryMethod.TELEGRAM_APP
        "sms" -> LoginCodeDeliveryMethod.SMS
        "voice_call" -> LoginCodeDeliveryMethod.VOICE_CALL
        "flash_call" -> LoginCodeDeliveryMethod.FLASH_CALL
        "missed_call" -> LoginCodeDeliveryMethod.MISSED_CALL
        "firebase_sms" -> LoginCodeDeliveryMethod.FIREBASE_SMS
        "fragment_sms" -> LoginCodeDeliveryMethod.FRAGMENT_SMS
        "sms_word" -> LoginCodeDeliveryMethod.SMS_WORD
        "sms_phrase" -> LoginCodeDeliveryMethod.SMS_PHRASE
        "email_code" -> LoginCodeDeliveryMethod.EMAIL_CODE
        "email_setup_required" -> LoginCodeDeliveryMethod.EMAIL_SETUP_REQUIRED
        else -> null
    }
}.getOrNull()

/** Clear the display-only method whenever the Rust authorization stage leaves code entry. */
internal fun deliveryAfterAuthorizationChange(
    current: LoginCodeDeliveryMethod?,
    authorization: AuthorizationUiStep,
): LoginCodeDeliveryMethod? = current.takeIf {
    authorization == AuthorizationUiStep.WaitingForCode
}

/** Phone submission starts a fresh code request, even if Telegram rejects the request. */
internal fun deliveryAfterPhoneRequest(): LoginCodeDeliveryMethod? = null

/** Ignore the bridge's default pre-restore snapshot until Core marks it authoritative. */
internal fun authorizationFromRestoreSnapshot(
    current: AuthorizationUiStep,
    snapshot: AuthorizationUiStep,
    restoreComplete: Boolean,
): AuthorizationUiStep = if (restoreComplete) snapshot else current

internal fun authorizationUiCanOpen(
    libraryLoaded: Boolean,
    sessionRestoreComplete: Boolean,
): Boolean = libraryLoaded && sessionRestoreComplete

/** Keep the method across retryable code/network errors; clear expired login-token metadata. */
internal fun deliveryAfterAuthorizationError(
    current: LoginCodeDeliveryMethod?,
    authorization: AuthorizationUiStep,
    error: String,
): LoginCodeDeliveryMethod? {
    if (authorization != AuthorizationUiStep.WaitingForCode) return null
    val normalized = error.uppercase()
    return if (
        listOf("PHONE_CODE_EXPIRED", "PHONE_CODE_HASH_EMPTY", "PHONE_CODE_HASH_INVALID", "AUTH_RESTART")
            .any { normalized.contains(it) }
    ) null else current
}

/** A neutral fallback avoids claiming a method when Telegram omitted or changed metadata. */
internal fun authorizationCodeDeliveryLabel(method: LoginCodeDeliveryMethod?): String = when (method) {
    LoginCodeDeliveryMethod.TELEGRAM_APP -> "Telegram selected in-app code delivery."
    LoginCodeDeliveryMethod.SMS -> "Telegram selected SMS delivery."
    LoginCodeDeliveryMethod.VOICE_CALL -> "Telegram selected a voice call."
    LoginCodeDeliveryMethod.FLASH_CALL -> "Telegram selected flash-call verification."
    LoginCodeDeliveryMethod.MISSED_CALL -> "Telegram selected missed-call verification."
    LoginCodeDeliveryMethod.FIREBASE_SMS -> "Telegram selected Firebase SMS delivery."
    LoginCodeDeliveryMethod.FRAGMENT_SMS -> "Telegram selected Fragment SMS delivery."
    LoginCodeDeliveryMethod.SMS_WORD -> "Telegram selected an SMS containing a word."
    LoginCodeDeliveryMethod.SMS_PHRASE -> "Telegram selected an SMS containing a phrase."
    LoginCodeDeliveryMethod.EMAIL_CODE -> "Telegram selected email delivery."
    LoginCodeDeliveryMethod.EMAIL_SETUP_REQUIRED -> "Telegram requires email setup before code delivery."
    null -> "Telegram did not provide a delivery method. Enter the code using the instructions you received."
}
