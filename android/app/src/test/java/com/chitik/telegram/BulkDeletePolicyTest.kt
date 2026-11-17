package com.chitik.telegram

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BulkDeletePolicyTest {
    @Test
    fun outgoingMessagesRemainDeletableWithoutAdminCapability() {
        val outgoing = listOf(message("1", outgoing = true), message("2", outgoing = true))

        assertTrue(canBulkDeleteMessages(2, outgoing, canDeleteMessages = false))
        assertTrue(canBulkDeleteMessages(2, outgoing, canDeleteMessages = null))
    }

    @Test
    fun explicitDeleteCapabilityAllowsIncomingAndMixedSelections() {
        assertTrue(
            canBulkDeleteMessages(
                1,
                listOf(message("3", outgoing = false)),
                canDeleteMessages = true,
            ),
        )
        assertTrue(
            canBulkDeleteMessages(
                2,
                listOf(message("4", outgoing = true), message("5", outgoing = false)),
                canDeleteMessages = true,
            ),
        )
    }

    @Test
    fun incomingOrMixedSelectionsFailClosedWithoutExplicitCapability() {
        val incoming = listOf(message("6", outgoing = false))
        val mixed = listOf(message("7", outgoing = true), message("8", outgoing = false))

        assertFalse(canBulkDeleteMessages(1, incoming, canDeleteMessages = false))
        assertFalse(canBulkDeleteMessages(1, incoming, canDeleteMessages = null))
        assertFalse(canBulkDeleteMessages(2, mixed, canDeleteMessages = false))
        assertFalse(canBulkDeleteMessages(2, mixed, canDeleteMessages = null))
    }

    @Test
    fun emptyOrPartiallyResolvedSelectionCannotBeDeleted() {
        assertFalse(canBulkDeleteMessages(0, emptyList(), canDeleteMessages = true))
        assertFalse(
            canBulkDeleteMessages(
                2,
                listOf(message("9", outgoing = true)),
                canDeleteMessages = true,
            ),
        )
    }

    private fun message(id: String, outgoing: Boolean) = ConversationMessageUi(
        id = id,
        chatId = "7",
        text = "message $id",
        timestamp = "",
        isOutgoing = outgoing,
    )
}
