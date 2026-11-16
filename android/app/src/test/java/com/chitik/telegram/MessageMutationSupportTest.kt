package com.chitik.telegram

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MessageMutationSupportTest {
    @Test
    fun editComposerRequiresServerMessageText() {
        assertTrue(canEditMessageText(message(text = "old text")))
        assertFalse(canEditMessageText(message(text = "  ")))
        assertFalse(canEditMessageText(message(text = "incoming", outgoing = false)))
    }

    @Test
    fun deleteForMeIsGatedForChannelPeerIds() {
        assertTrue(supportsDeleteForMe(42L))
        assertTrue(supportsDeleteForMe(-42L))
        assertFalse(supportsDeleteForMe(-1_000_000_000_042L))
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

    @Test
    fun sessionReplacementMarksInFlightMutationUnconfirmed() {
        val interrupted = messageMutationStateAfterSessionChange(
            pending = true,
            currentError = null,
            reason = "Session changed",
        )
        assertFalse(interrupted.pending)
        assertTrue(interrupted.error == "Session changed")

        val idle = messageMutationStateAfterSessionChange(
            pending = false,
            currentError = "Keep this error",
            reason = "Session changed",
        )
        assertFalse(idle.pending)
        assertTrue(idle.error == "Keep this error")
    }

    private fun message(text: String, outgoing: Boolean = true) = ConversationMessageUi(
        id = "12",
        chatId = "-1001",
        text = text,
        timestamp = "",
        isOutgoing = outgoing,
    )
}
