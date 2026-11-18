package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class ConversationMessageGroupingTest {
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
    ) = ConversationMessageUi(
        id = id,
        chatId = "chat",
        text = id,
        timestamp = "10:00",
        dateSeparator = date,
        isOutgoing = outgoing,
        senderId = senderId,
        serviceAction = if (service) ServiceActionUi.Unknown else null,
    )
}
