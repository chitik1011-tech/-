package com.chitik.telegram

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SessionNavigationPolicyTest {
    @Test
    fun initialNonReadyStatePreservesProcessRestoredNavigation() {
        assertFalse(
            shouldResetNavigationAfterAuthorizationChange(
                wasAuthorizationReady = false,
                isAuthorizationReady = false,
            ),
        )
    }

    @Test
    fun clearsNavigationWhenAnAuthorizedSessionLosesAuthorization() {
        assertTrue(
            shouldResetNavigationAfterAuthorizationChange(
                wasAuthorizationReady = true,
                isAuthorizationReady = false,
            ),
        )
    }

    @Test
    fun doesNotClearWhileAuthorizationRemainsReady() {
        assertFalse(
            shouldResetNavigationAfterAuthorizationChange(
                wasAuthorizationReady = true,
                isAuthorizationReady = true,
            ),
        )
    }
}
