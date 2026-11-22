package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MessagePinActionPresentationTest {
    @Test
    fun eligibleMessagesOfferPinOrUnpinOnlyWhenPermissionIsAvailable() {
        val message = message()

        val pin = messagePinActionPresentation(
            message = message,
            canPin = true,
            isPinned = false,
            isPending = false,
            mutationPending = false,
        )
        assertEquals("Закрепить", pin.label)
        assertTrue(pin.enabled)

        val unpin = messagePinActionPresentation(
            message = message,
            canPin = true,
            isPinned = true,
            isPending = false,
            mutationPending = false,
        )
        assertEquals("Открепить", unpin.label)
        assertTrue(unpin.enabled)

        val forbidden = messagePinActionPresentation(
            message = message,
            canPin = false,
            isPinned = false,
            isPending = false,
            mutationPending = false,
        )
        assertNull(forbidden.label)
        assertFalse(forbidden.enabled)
    }

    @Test
    fun pinActionIsDisabledWhilePinOrMessageMutationIsPending() {
        val pendingPin = messagePinActionPresentation(message(), true, false, true, false)
        assertEquals("Закрепить", pendingPin.label)
        assertTrue(pendingPin.pending)
        assertFalse(pendingPin.enabled)

        val pendingMutation = messagePinActionPresentation(message(), true, false, false, true)
        assertFalse(pendingMutation.enabled)
    }

    @Test
    fun nonServerAndServiceMessagesDoNotOfferPinActions() {
        val invalidId = message(id = "local-1")
        val serviceMessage = message().copy(serviceAction = ServiceActionUi.Unknown)

        listOf(invalidId, serviceMessage).forEach { candidate ->
            val presentation = messagePinActionPresentation(candidate, true, false, false, false)
            assertNull(presentation.label)
            assertFalse(presentation.enabled)
        }
    }

    private fun message(id: String = "12") = ConversationMessageUi(
        id = id,
        chatId = "-1001",
        text = "Message",
        timestamp = "10:00",
        isOutgoing = false,
    )
}
