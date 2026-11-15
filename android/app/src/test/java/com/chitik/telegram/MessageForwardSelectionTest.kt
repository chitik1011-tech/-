package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class MessageForwardSelectionTest {
    @Test
    fun selectedIdsFollowConversationOrderRatherThanTapOrder() {
        val messages = listOf(
            message("10"),
            message("11"),
            message("12"),
        )
        assertEquals(listOf(10L, 12L), selectedForwardMessageIds(messages, setOf(12L, 10L)))
    }

    @Test
    fun malformedIdsAreSkippedAndDuplicateRowsDoNotDuplicateForwarding() {
        val messages = listOf(message("5"), message("bad"), message("5"))
        assertEquals(listOf(5L), selectedForwardMessageIds(messages, setOf(5L)))
    }

    private fun message(id: String) = ConversationMessageUi(
        id = id,
        chatId = "1",
        text = "",
        timestamp = "",
        isOutgoing = false,
    )
}
