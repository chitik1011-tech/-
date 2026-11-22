package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConversationMessageGroupingTest {
    @Test
    fun unreadBoundaryStartsAtFirstUnreadIncomingMessage() {
        val messages = listOf(
            message("1", senderId = 10),
            message("2", senderId = 10, outgoing = true, unread = true),
            message("3", senderId = 10, unread = true),
            message("4", senderId = 11, unread = true),
        )

        assertEquals(2, firstUnreadIncomingMessageIndex(messages))
        assertEquals(-1, firstUnreadIncomingMessageIndex(messages.filterNot { it.isUnread }))
    }

    @Test
    fun unreadPolicyUsesKnownInboxBoundaryAndNeverMarksOutgoingMessages() {
        assertFalse(isUnreadIncomingMessage(messageId = 10, isOutgoing = false, inboxReadBoundary = 10))
        assertTrue(isUnreadIncomingMessage(messageId = 11, isOutgoing = false, inboxReadBoundary = 10))
        assertFalse(isUnreadIncomingMessage(messageId = 11, isOutgoing = true, inboxReadBoundary = 10))
        assertFalse(isUnreadIncomingMessage(messageId = 11, isOutgoing = false, inboxReadBoundary = 0))
        assertFalse(isUnreadIncomingMessage(messageId = 11, isOutgoing = false, inboxReadBoundary = null))
    }

    @Test
    fun consecutiveMessagesFromSameIncomingSenderShareGroup() {
        val messages = listOf(message("1", senderId = 10), message("2", senderId = 10))

        assertEquals(ConversationMessageGroupPosition(firstInGroup = true, lastInGroup = false),
            conversationMessageGroupPosition(messages, 0))
        assertEquals(ConversationMessageGroupPosition(firstInGroup = false, lastInGroup = true),
            conversationMessageGroupPosition(messages, 1))
    }

    @Test
    fun senderDirectionDateAndServiceMessagesBreakGroups() {
        val first = message("1", senderId = 10)
        val variants = listOf(
            message("2", senderId = 11),
            message("2", senderId = 0),
            message("2", senderId = 10, outgoing = true),
            message("2", senderId = 10, date = "Tuesday"),
            message("2", senderId = 10, unread = true),
            message("2", senderId = 10, service = true),
        )

        variants.forEach { second ->
            assertEquals(ConversationMessageGroupPosition(firstInGroup = true, lastInGroup = true),
                conversationMessageGroupPosition(listOf(first, second), 0))
            assertEquals(ConversationMessageGroupPosition(firstInGroup = true, lastInGroup = true),
                conversationMessageGroupPosition(listOf(first, second), 1))
        }
    }

    private fun message(
        id: String,
        senderId: Long,
        outgoing: Boolean = false,
        date: String = "Monday",
        service: Boolean = false,
        unread: Boolean = false,
    ) = ConversationMessageUi(
        id = id,
        chatId = "chat",
        text = id,
        timestamp = "10:00",
        dateSeparator = date,
        isOutgoing = outgoing,
        isUnread = unread,
        senderId = senderId,
        serviceAction = if (service) ServiceActionUi.Unknown else null,
    )
}
