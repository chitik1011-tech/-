package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

class AuthorizationCodeDeliveryTest {
    @Test
    fun parsesEveryClosedWireValueWithoutGuessing() {
        val cases = mapOf(
            "telegram_app" to LoginCodeDeliveryMethod.TELEGRAM_APP,
            "sms" to LoginCodeDeliveryMethod.SMS,
            "voice_call" to LoginCodeDeliveryMethod.VOICE_CALL,
            "flash_call" to LoginCodeDeliveryMethod.FLASH_CALL,
            "missed_call" to LoginCodeDeliveryMethod.MISSED_CALL,
            "firebase_sms" to LoginCodeDeliveryMethod.FIREBASE_SMS,
            "fragment_sms" to LoginCodeDeliveryMethod.FRAGMENT_SMS,
            "sms_word" to LoginCodeDeliveryMethod.SMS_WORD,
            "sms_phrase" to LoginCodeDeliveryMethod.SMS_PHRASE,
            "email_code" to LoginCodeDeliveryMethod.EMAIL_CODE,
            "email_setup_required" to LoginCodeDeliveryMethod.EMAIL_SETUP_REQUIRED,
        )

        for ((wireValue, expected) in cases) {
            assertEquals(
                expected,
                parseLoginCodeDeliveryEvent(
                    """{"type":"authorization_code_delivery_changed","payload":"$wireValue"}""",
                ),
            )
        }
    }

    @Test
    fun unknownAndMissingValuesHaveNeutralFallback() {
        assertNull(parseLoginCodeDeliveryEvent("""{"type":"authorization_code_delivery_changed","payload":"future_kind"}"""))
        assertNull(parseLoginCodeDeliveryEvent("""{"type":"authorization_code_delivery_changed","payload":null}"""))
        assertNull(parseLoginCodeDeliveryEvent("not json"))
        val label = authorizationCodeDeliveryLabel(null)
        assertEquals(
            "Telegram did not provide a delivery method. Enter the code using the instructions you received.",
            label,
        )
        assert(!label.contains("SMS", ignoreCase = true))
        assert(!label.contains("in-app", ignoreCase = true))
    }

    @Test
    fun eachKnownMethodHasAnExhaustiveUserFacingLabel() {
        val labels = LoginCodeDeliveryMethod.entries.associateWith(::authorizationCodeDeliveryLabel)
        assertEquals(LoginCodeDeliveryMethod.entries.size, labels.size)
        assertEquals(LoginCodeDeliveryMethod.entries.size, labels.values.toSet().size)
        assertEquals("Telegram selected SMS delivery.", labels.getValue(LoginCodeDeliveryMethod.SMS))
    }

    @Test
    fun invalidCodeAndFloodWaitPreserveDeliveryButExpiredTokenClearsIt() {
        val delivery = LoginCodeDeliveryMethod.TELEGRAM_APP
        assertSame(
            delivery,
            deliveryAfterAuthorizationError(
                delivery,
                AuthorizationUiStep.WaitingForCode,
                "invalid Telegram login code",
            ),
        )
        assertSame(
            delivery,
            deliveryAfterAuthorizationError(
                delivery,
                AuthorizationUiStep.WaitingForCode,
                "FLOOD_WAIT_30",
            ),
        )
        for (expiredError in listOf("PHONE_CODE_EXPIRED", "PHONE_CODE_HASH_INVALID", "AUTH_RESTART")) {
            assertNull(
                deliveryAfterAuthorizationError(
                    delivery,
                    AuthorizationUiStep.WaitingForCode,
                    expiredError,
                ),
            )
        }
    }

    @Test
    fun phoneRequestAndAuthorizationStageChangesClearDeliveryMetadata() {
        val delivery = LoginCodeDeliveryMethod.SMS
        assertNull(deliveryAfterPhoneRequest())
        assertSame(
            delivery,
            deliveryAfterAuthorizationChange(delivery, AuthorizationUiStep.WaitingForCode),
        )
        for (step in AuthorizationUiStep.entries - AuthorizationUiStep.WaitingForCode) {
            assertNull(deliveryAfterAuthorizationChange(delivery, step))
        }
    }

    @Test
    fun preRestoreClosedSnapshotCannotReplaceAuthorization() {
        val current = AuthorizationUiStep.Starting
        assertEquals(
            current,
            authorizationFromRestoreSnapshot(
                current,
                AuthorizationUiStep.Closed,
                restoreComplete = false,
            ),
        )
        assertEquals(
            AuthorizationUiStep.Ready,
            authorizationFromRestoreSnapshot(
                current,
                AuthorizationUiStep.Ready,
                restoreComplete = true,
            ),
        )
        assertEquals(
            AuthorizationUiStep.Closed,
            authorizationFromRestoreSnapshot(
                current,
                AuthorizationUiStep.Closed,
                restoreComplete = true,
            ),
        )
    }

    @Test
    fun authorizationUiWaitsForCoreStateRestoreCompletion() {
        assertEquals(false, authorizationUiCanOpen(libraryLoaded = false, sessionRestoreComplete = true))
        assertEquals(false, authorizationUiCanOpen(libraryLoaded = true, sessionRestoreComplete = false))
        assertEquals(true, authorizationUiCanOpen(libraryLoaded = true, sessionRestoreComplete = true))
    }
}
