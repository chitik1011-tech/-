package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Test

class PendingNotificationRouteTest {
    @Test
    fun freshValidTapTakesPriorityOverSavedRoute() {
        assertEquals(22L, restorePendingNotificationChatId(11L, 22L))
    }

    @Test
    fun savedRouteSurvivesAbsentOrMalformedFreshRoute() {
        assertEquals(11L, restorePendingNotificationChatId(11L, null))
        assertEquals(11L, restorePendingNotificationChatId(11L, 0L))
    }

    @Test
    fun freshRouteIsUsedWhenSavedRouteIsInvalid() {
        assertEquals(22L, restorePendingNotificationChatId(0L, 22L))
    }

    @Test
    fun invalidRoutesDoNotCreatePendingState() {
        assertEquals(null, restorePendingNotificationChatId(null, null))
        assertEquals(null, restorePendingNotificationChatId(0L, 0L))
    }
}
