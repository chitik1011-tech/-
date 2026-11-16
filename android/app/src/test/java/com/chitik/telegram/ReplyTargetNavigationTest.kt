package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class ReplyTargetNavigationTest {
    @Test
    fun findsLoadedOriginalMessageForDirectJump() {
        val messages = listOf(message("17"), message("18"))
        assertEquals(1, replyTargetIndex(messages, "18"))
    }

    @Test
    fun missingOrDeletedOriginalRequestsHistoryAndKeepsUnavailableFallback() {
        val messages = listOf(message("17"))
        assertEquals(-1, replyTargetIndex(messages, "3"))
    }

    private fun message(id: String) = ConversationMessageUi(
        id = id,
        chatId = "2",
        text = "",
        timestamp = "",
        isOutgoing = false,
    )
}
