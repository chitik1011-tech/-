package com.chitik.telegram.notifications

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationPermissionRequestGateTest {
    @Test
    fun allowsOneOutstandingRequestAndCanRetryAfterCompletion() {
        val gate = NotificationPermissionRequestGate()

        assertTrue(gate.begin())
        assertFalse(gate.begin())

        gate.finish()

        assertTrue(gate.begin())
    }

    @Test
    fun finishIsSafeWhenLaunchFailedBeforeRequestWasPending() {
        val gate = NotificationPermissionRequestGate()

        gate.finish()

        assertTrue(gate.begin())
    }
}
