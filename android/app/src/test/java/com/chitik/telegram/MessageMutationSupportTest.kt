package com.chitik.telegram

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MessageMutationSupportTest {
    @Test
    fun editComposerRequiresServerMessageText() {
        assertTrue(canEditMessageText(message(text = "old text")))
        assertFalse(canEditMessageText(message(text = "  ")))
    }

    private fun message(text: String) = ConversationMessageUi(
        id = "12",
        chatId = "-1001",
        text = text,
        timestamp = "",
        isOutgoing = true,
    )
}
