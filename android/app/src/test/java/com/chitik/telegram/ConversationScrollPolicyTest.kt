package com.chitik.telegram

import org.junit.Assert.assertFalse
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ConversationScrollPolicyTest {
    @Test
    fun firstOpenScrollsToLatestAndClearsNewMessageAffordance() {
        val decision = decideConversationScroll(
            hasOpenedConversation = false,
            isNearLatest = false,
            latestMessageChanged = true,
        )

        assertTrue(decision.scrollToLatest)
        assertFalse(decision.showNewMessages!!)
    }

    @Test
    fun incomingMessageWhileReadingHistoryPreservesPositionAndShowsAffordance() {
        val decision = decideConversationScroll(
            hasOpenedConversation = true,
            isNearLatest = false,
            latestMessageChanged = true,
        )

        assertFalse(decision.scrollToLatest)
        assertTrue(decision.showNewMessages!!)
    }

    @Test
    fun incomingMessageNearLatestScrollsAndDismissesAffordance() {
        val decision = decideConversationScroll(
            hasOpenedConversation = true,
            isNearLatest = true,
            latestMessageChanged = true,
        )

        assertTrue(decision.scrollToLatest)
        assertFalse(decision.showNewMessages!!)
    }

    @Test
    fun existingAffordanceIsPreservedUntilLatestMessageIsVisible() {
        val unchanged = decideConversationScroll(
            hasOpenedConversation = true,
            isNearLatest = false,
            latestMessageChanged = false,
        )
        assertNull(unchanged.showNewMessages)

        assertFalse(isLatestConversationMessageVisible(9, setOf(6, 7, 8)))
        assertTrue(isLatestConversationMessageVisible(9, setOf(8, 9)))
        assertTrue(isLatestConversationMessageVisible(-1, emptySet()))
    }

    @Test
    fun countsEveryMessageAppendedAfterTheLastObservedMessage() {
        assertEquals(3, newMessagesAfterAnchor(listOf("10", "11", "12", "13", "14"), "11"))
    }

    @Test
    fun missingOrUnsetAnchorUsesSafeFallback() {
        assertEquals(0, newMessagesAfterAnchor(listOf("1", "2"), null))
        assertEquals(1, newMessagesAfterAnchor(listOf("1", "2"), "deleted-anchor"))
        assertEquals(0, newMessagesAfterAnchor(emptyList(), "1"))
    }
}
