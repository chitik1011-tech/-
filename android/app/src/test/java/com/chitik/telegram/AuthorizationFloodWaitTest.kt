package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AuthorizationFloodWaitTest {
    @Test
    fun readsOnlyRustFloodWaitErrorShape() {
        assertEquals(
            47,
            authorizationFloodWaitSeconds("Telegram FLOOD_WAIT: retry after 47 seconds"),
        )
        assertEquals(
            1,
            authorizationFloodWaitSeconds("Telegram FLOOD_WAIT: retry after 1 second"),
        )
        assertNull(authorizationFloodWaitSeconds("FLOOD_WAIT_invalid"))
        assertNull(authorizationFloodWaitSeconds("PHONE_NUMBER_FLOOD"))
        assertNull(authorizationFloodWaitSeconds("retry after 0 seconds"))
    }

    @Test
    fun authInputsStayDisabledUntilTelegramWaitHasElapsed() {
        assertFalse(authorizationInputEnabled(isLoading = false, retryAfterSeconds = 47))
        assertFalse(authorizationInputEnabled(isLoading = true, retryAfterSeconds = 0))
        assertTrue(authorizationInputEnabled(isLoading = false, retryAfterSeconds = 0))
        assertFalse(
            canSubmitAuthorizationInput(
                currentStep = AuthorizationUiStep.WaitingForCode,
                expectedStep = AuthorizationUiStep.WaitingForCode,
                isLoading = false,
                retryAfterSeconds = 47,
            ),
        )
    }

    @Test
    fun generatedWaitCopyUpdatesAndHandlesSingularSecond() {
        assertEquals(
            "Telegram asked you to wait 2 seconds before trying again.",
            authorizationFloodWaitMessage(2),
        )
        assertEquals(
            "Telegram asked you to wait 1 second before trying again.",
            authorizationFloodWaitMessage(1),
        )
    }
}
