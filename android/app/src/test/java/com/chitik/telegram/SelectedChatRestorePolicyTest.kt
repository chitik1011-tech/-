package com.chitik.telegram

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SelectedChatRestorePolicyTest {
    @Test
    fun restoredSelectedChatWithMissingCacheLoadsAfterAuthorizedRestore() {
        assertTrue(
            shouldLoadRestoredSelectedChat(
                activityStateRestored = true,
                sessionRestoreComplete = true,
                authorizationReady = true,
                selectedChatId = -42L,
                hasCachedMessages = false,
                explicitlyLoadedForChat = false,
                hasPendingLink = false,
            ),
        )
    }

    @Test
    fun restoredSelectedChatWithCachedMessagesDoesNotReload() {
        assertFalse(
            shouldLoadRestoredSelectedChat(
                activityStateRestored = true,
                sessionRestoreComplete = true,
                authorizationReady = true,
                selectedChatId = -42L,
                hasCachedMessages = true,
                explicitlyLoadedForChat = false,
                hasPendingLink = false,
            ),
        )
    }

    @Test
    fun waitsForAuthoritativeAuthorizedSessionRestore() {
        assertFalse(
            shouldLoadRestoredSelectedChat(
                activityStateRestored = false,
                sessionRestoreComplete = true,
                authorizationReady = true,
                selectedChatId = 42L,
                hasCachedMessages = false,
                explicitlyLoadedForChat = false,
                hasPendingLink = false,
            ),
        )
        assertFalse(
            shouldLoadRestoredSelectedChat(
                activityStateRestored = true,
                sessionRestoreComplete = false,
                authorizationReady = true,
                selectedChatId = 42L,
                hasCachedMessages = false,
                explicitlyLoadedForChat = false,
                hasPendingLink = false,
            ),
        )
        assertFalse(
            shouldLoadRestoredSelectedChat(
                activityStateRestored = true,
                sessionRestoreComplete = true,
                authorizationReady = false,
                selectedChatId = 42L,
                hasCachedMessages = false,
                explicitlyLoadedForChat = false,
                hasPendingLink = false,
            ),
        )
    }

    @Test
    fun doesNotDuplicateExplicitRouteLoadsOrLoadRootRoute() {
        assertFalse(
            shouldLoadRestoredSelectedChat(
                activityStateRestored = true,
                sessionRestoreComplete = true,
                authorizationReady = true,
                selectedChatId = 42L,
                hasCachedMessages = false,
                explicitlyLoadedForChat = true,
                hasPendingLink = false,
            ),
        )
        assertFalse(
            shouldLoadRestoredSelectedChat(
                activityStateRestored = true,
                sessionRestoreComplete = true,
                authorizationReady = true,
                selectedChatId = 0L,
                hasCachedMessages = false,
                explicitlyLoadedForChat = false,
                hasPendingLink = false,
            ),
        )
    }

    @Test
    fun pendingDeepLinkSuppressesOldSelectedChatReload() {
        assertFalse(
            shouldLoadRestoredSelectedChat(
                activityStateRestored = true,
                sessionRestoreComplete = true,
                authorizationReady = true,
                selectedChatId = 42L,
                hasCachedMessages = false,
                explicitlyLoadedForChat = false,
                hasPendingLink = true,
            ),
        )
    }
}
