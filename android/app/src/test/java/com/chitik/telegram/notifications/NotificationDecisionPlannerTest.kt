package com.chitik.telegram.notifications

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationDecisionPlannerTest {
    private fun identity(
        session: Long = 1L,
        chatId: Long = 10L,
        messageId: Long = 20L,
    ) = NotificationDecisionPlanner.Identity(session, chatId, messageId)

    @Test
    fun notifiesOncePerServerMessageAndSeparatesChats() {
        val planner = NotificationDecisionPlanner()
        planner.reset(1L)

        assertTrue(planner.shouldNotify(identity(), outgoing = false, liveUpdate = true))
        assertFalse(planner.shouldNotify(identity(), outgoing = false, liveUpdate = true))
        assertTrue(
            planner.shouldNotify(
                identity(chatId = 11L),
                outgoing = false,
                liveUpdate = true,
            ),
        )
    }

    @Test
    fun rejectsStaleSessionOutgoingHistoryAndInvalidIdentities() {
        val planner = NotificationDecisionPlanner()
        planner.reset(2L)

        assertFalse(planner.shouldNotify(identity(session = 1L), outgoing = false, liveUpdate = true))
        assertFalse(planner.shouldNotify(identity(), outgoing = true, liveUpdate = true))
        assertFalse(planner.shouldNotify(identity(), outgoing = false, liveUpdate = false))
        assertFalse(planner.shouldNotify(identity(chatId = 0L), outgoing = false, liveUpdate = true))
        assertFalse(planner.shouldNotify(identity(messageId = 0L), outgoing = false, liveUpdate = true))
        assertFalse(planner.shouldNotify(identity(messageId = -1L), outgoing = false, liveUpdate = true))
    }

    @Test
    fun suppressesCurrentForegroundChatAndConsumesItsIdentity() {
        val planner = NotificationDecisionPlanner()
        planner.reset(1L)
        planner.updateVisibility(foreground = true, chatId = 10L)

        assertFalse(planner.shouldNotify(identity(), outgoing = false, liveUpdate = true))
        assertTrue(
            planner.shouldNotify(
                identity(chatId = 11L),
                outgoing = false,
                liveUpdate = true,
            ),
        )

        planner.updateVisibility(foreground = false, chatId = null)
        assertFalse(planner.shouldNotify(identity(), outgoing = false, liveUpdate = true))
    }

    @Test
    fun resetClearsDeduplicationAndRejectsOldSessionEvents() {
        val planner = NotificationDecisionPlanner()
        planner.reset(1L)
        assertTrue(planner.shouldNotify(identity(), outgoing = false, liveUpdate = true))

        planner.reset(2L)

        assertFalse(planner.shouldNotify(identity(), outgoing = false, liveUpdate = true))
        assertTrue(
            planner.shouldNotify(
                identity(session = 2L),
                outgoing = false,
                liveUpdate = true,
            ),
        )
    }

    @Test
    fun deduplicationRemainsBounded() {
        val planner = NotificationDecisionPlanner(capacity = 2)
        planner.reset(1L)

        assertTrue(planner.shouldNotify(identity(messageId = 1L), outgoing = false, liveUpdate = true))
        assertTrue(planner.shouldNotify(identity(messageId = 2L), outgoing = false, liveUpdate = true))
        assertTrue(planner.shouldNotify(identity(messageId = 3L), outgoing = false, liveUpdate = true))
        assertTrue(planner.shouldNotify(identity(messageId = 1L), outgoing = false, liveUpdate = true))
    }
}
