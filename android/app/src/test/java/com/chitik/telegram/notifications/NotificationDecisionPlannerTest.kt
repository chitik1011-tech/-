package com.chitik.telegram.notifications

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationDecisionPlannerTest {
    private fun key(session: Long = 1, chat: Long = 10, message: Long = 20) =
        NotificationDecisionPlanner.Identity(session, chat, message)

    @Test fun incomingIsOncePerSessionAndChat() {
        val planner = NotificationDecisionPlanner()
        planner.reset(1)
        assertTrue(planner.shouldNotify(key(), outgoing = false, liveUpdate = true))
        assertFalse(planner.shouldNotify(key(), outgoing = false, liveUpdate = true))
        assertTrue(planner.shouldNotify(key(chat = 11), outgoing = false, liveUpdate = true))
        assertFalse(planner.shouldNotify(key(session = 2), outgoing = false, liveUpdate = true))
    }

    @Test fun ownHistoryInvalidAndOldSessionAreIgnored() {
        val planner = NotificationDecisionPlanner()
        planner.reset(1)
        assertFalse(planner.shouldNotify(key(), outgoing = true, liveUpdate = true))
        assertFalse(planner.shouldNotify(key(), outgoing = false, liveUpdate = false))
        assertFalse(planner.shouldNotify(key(message = -1), outgoing = false, liveUpdate = true))
        assertFalse(planner.shouldNotify(key(chat = 0), outgoing = false, liveUpdate = true))
        planner.reset(2)
        assertFalse(planner.shouldNotify(key(), outgoing = false, liveUpdate = true))
        assertTrue(planner.shouldNotify(key(session = 2), outgoing = false, liveUpdate = true))
    }

    @Test fun foregroundSuppressionConsumesIdentityButOtherChatsNotify() {
        val planner = NotificationDecisionPlanner()
        planner.reset(1)
        planner.updateVisibility(true, 10)
        assertFalse(planner.shouldNotify(key(), outgoing = false, liveUpdate = true))
        assertTrue(planner.shouldNotify(key(chat = 11), outgoing = false, liveUpdate = true))
        planner.updateVisibility(false, null)
        assertFalse(planner.shouldNotify(key(), outgoing = false, liveUpdate = true))
    }

    @Test fun readDoesNotEnableReplayNotification() {
        val planner = NotificationDecisionPlanner()
        planner.reset(1)
        assertTrue(planner.shouldNotify(key(), outgoing = false, liveUpdate = true))
        planner.markRead(1, 10, 20)
        assertFalse(planner.shouldNotify(key(), outgoing = false, liveUpdate = true))
    }

    @Test fun resetInvalidatesOldAccountAndClearsState() {
        val planner = NotificationDecisionPlanner()
        planner.reset(1)
        assertTrue(planner.shouldNotify(key(), outgoing = false, liveUpdate = true))
        planner.reset(null)
        assertFalse(planner.shouldNotify(key(), outgoing = false, liveUpdate = true))
        planner.reset(2)
        assertTrue(planner.shouldNotify(key(session = 2), outgoing = false, liveUpdate = true))
    }
}
