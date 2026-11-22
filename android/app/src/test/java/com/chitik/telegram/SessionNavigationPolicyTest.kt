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

    @Test
    fun clearsSavedChatWhenRestoredSessionBelongsToAnotherAccount() {
        assertTrue(
            shouldClearChatSelectionForAccountMismatch(
                selectedChatId = -41L,
                selectedChatAccountUserId = 100L,
                accountIdentityResolved = true,
                activeAccountUserId = 200L,
            ),
        )
    }

    @Test
    fun preservesChatOnlyForTheSameResolvedAccount() {
        assertFalse(
            shouldClearChatSelectionForAccountMismatch(
                selectedChatId = -41L,
                selectedChatAccountUserId = 100L,
                accountIdentityResolved = true,
                activeAccountUserId = 100L,
            ),
        )
        assertFalse(
            shouldClearChatSelectionForAccountMismatch(
                selectedChatId = -41L,
                selectedChatAccountUserId = 100L,
                accountIdentityResolved = false,
                activeAccountUserId = null,
            ),
        )
    }

    @Test
    fun clearsLegacyUnscopedSelectionAfterIdentityResolves() {
        assertTrue(
            shouldClearChatSelectionForAccountMismatch(
                selectedChatId = 41L,
                selectedChatAccountUserId = null,
                accountIdentityResolved = true,
                activeAccountUserId = 100L,
            ),
        )
    }
}
