package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Test

class PendingNotificationRouteTest {
    @Test
    fun freshValidTapTakesPriorityOverSavedRoute() {
        assertEquals(
            TelegramNotificationRoute(22L, 200L),
            restorePendingNotificationRoute(
                savedChatId = 11L,
                savedAccountUserId = 100L,
                freshIntentRoute = TelegramNotificationRoute(22L, 200L),
            ),
        )
    }

    @Test
    fun savedAccountScopedRouteSurvivesAnAbsentFreshRoute() {
        assertEquals(
            TelegramNotificationRoute(11L, 100L),
            restorePendingNotificationRoute(11L, 100L, null),
        )
    }

    @Test
    fun invalidSavedRouteDoesNotCreatePendingState() {
        assertEquals(null, restorePendingNotificationRoute(0L, 100L, null))
        assertEquals(null, restorePendingNotificationRoute(11L, 0L, null))
    }

    @Test
    fun legacyChatOnlyRouteCannotBeRestoredWithoutAnAccountId() {
        assertEquals(null, restorePendingNotificationRoute(11L, null, null))
    }

    @Test
    fun invalidSavedRouteDoesNotOverrideFreshTap() {
        assertEquals(
            TelegramNotificationRoute(-5L, 200L),
            restorePendingNotificationRoute(0L, null, TelegramNotificationRoute(-5L, 200L)),
        )
    }

    @Test
    fun consumingNotificationClearsOnlyItsMatchingActivityIntentRoute() {
        val consumed = TelegramNotificationRoute(11L, 100L)

        assertEquals(true, shouldClearNotificationIntentAfterDismiss(consumed, consumed))
        assertEquals(false, shouldClearNotificationIntentAfterDismiss(consumed, TelegramNotificationRoute(12L, 100L)))
        assertEquals(false, shouldClearNotificationIntentAfterDismiss(consumed, null))
        assertEquals(false, shouldClearNotificationIntentAfterDismiss(null, consumed))
    }
    @Test
    fun incompleteSessionRestoreKeepsPendingRouteUntilOutcomeIsKnown() {
        assertEquals(
            PendingNotificationSessionDecision.WaitForRestore,
            decidePendingNotificationSessionRestore(
                restoreComplete = false,
                authorizationReady = false,
            ),
        )
        assertEquals(
            PendingNotificationSessionDecision.WaitForRestore,
            decidePendingNotificationSessionRestore(
                restoreComplete = false,
                authorizationReady = true,
            ),
        )
    }

    @Test
    fun completedReadyRestoreKeepsPendingRoute() {
        assertEquals(
            PendingNotificationSessionDecision.KeepPending,
            decidePendingNotificationSessionRestore(
                restoreComplete = true,
                authorizationReady = true,
            ),
        )
    }

    @Test
    fun completedNonReadyRestoreDiscardsPendingRoute() {
        assertEquals(
            PendingNotificationSessionDecision.Discard,
            decidePendingNotificationSessionRestore(
                restoreComplete = true,
                authorizationReady = false,
            ),
        )
    }
}
