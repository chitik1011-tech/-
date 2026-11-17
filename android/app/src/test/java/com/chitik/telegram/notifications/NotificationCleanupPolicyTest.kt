package com.chitik.telegram.notifications

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationCleanupPolicyTest {
    @Test
    fun clearsOnlyOnAParsedTransitionAwayFromAuthorized() {
        assertTrue(NotificationCleanupPolicy.shouldClearOnAuthorizationChange(true, true, false))
        assertFalse(NotificationCleanupPolicy.shouldClearOnAuthorizationChange(false, true, false))
        assertFalse(NotificationCleanupPolicy.shouldClearOnAuthorizationChange(true, false, false))
        assertFalse(NotificationCleanupPolicy.shouldClearOnAuthorizationChange(true, true, true))
    }

    @Test
    fun sameAuthorizedSessionHandleRecreationPreservesNotifications() {
        assertFalse(NotificationCleanupPolicy.shouldClearOnAuthorizationChange(true, true, true))
    }

    @Test
    fun restoredStateClearsOnlyAfterFirstAuthoritativeNonReadySnapshot() {
        assertFalse(
            NotificationCleanupPolicy.shouldClearOnRestoredAuthorizationState(
                restoreWasComplete = false,
                restoreIsComplete = false,
                isAuthorized = false,
            ),
        )
        assertTrue(
            NotificationCleanupPolicy.shouldClearOnRestoredAuthorizationState(
                restoreWasComplete = false,
                restoreIsComplete = true,
                isAuthorized = false,
            ),
        )
        assertFalse(
            NotificationCleanupPolicy.shouldClearOnRestoredAuthorizationState(
                restoreWasComplete = false,
                restoreIsComplete = true,
                isAuthorized = true,
            ),
        )
        assertFalse(
            NotificationCleanupPolicy.shouldClearOnRestoredAuthorizationState(
                restoreWasComplete = true,
                restoreIsComplete = true,
                isAuthorized = false,
            ),
        )
    }
}
