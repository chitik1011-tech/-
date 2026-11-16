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

    @Test
    fun onlyMatchingServerAcknowledgementCompletesMutation() {
        assertTrue(matchesMutationTarget(10, 42, 10, 42))
        assertFalse(matchesMutationTarget(10, 42, 11, 42))
        assertFalse(matchesMutationTarget(10, 42, 10, 43))
        assertFalse(mutationEventAcknowledges("message_edited", 10, 42, 10, 42))
        assertFalse(mutationEventAcknowledges("messages_deleted", 10, 42, 10, 42))
        assertTrue(mutationEventAcknowledges("message_mutation_succeeded", 10, 42, 10, 42))
        assertFalse(mutationEventAcknowledges("message_mutation_succeeded", 10, 42, 10, 43))
    }

    private fun message(text: String) = ConversationMessageUi(
        id = "12",
        chatId = "-1001",
        text = text,
        timestamp = "",
        isOutgoing = true,
    )
}
