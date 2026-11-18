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

    @Test
    fun selectedTextFollowsConversationOrderAndSkipsUnselectedRows() {
        val messages = listOf(
            message("10").copy(text = "first"),
            message("11").copy(text = "skip"),
            message("12").copy(text = "second"),
        )
        assertEquals("first\nsecond", selectedMessageText(messages, setOf(12L, 10L)))
    }

    @Test
    fun selectModeTapAddsAndRemovesMessages() {
        val firstTap = toggleSelectedMessageId(emptySet(), 17L)
        assertEquals(setOf(17L), firstTap)
        assertEquals(emptySet<Long>(), toggleSelectedMessageId(firstTap, 17L))
    }

    @Test
    fun unrelatedChatDeletionKeepsVisibleChatSelection() {
        val selected = setOf(17L, 18L)
        val visibleInOpenChat = setOf(17L, 18L, 19L)

        val result = pruneSelectedMessageIds(selected, visibleInOpenChat)

        assertEquals(selected, result.selectedIds)
        assertEquals(false, result.shouldExitSelectionMode)
    }

    @Test
    fun deletionPrunesOnlyMissingIdsAndExitsWhenSelectionIsEmpty() {
        val partial = pruneSelectedMessageIds(setOf(17L, 18L), setOf(18L, 19L))
        assertEquals(setOf(18L), partial.selectedIds)
        assertEquals(false, partial.shouldExitSelectionMode)

        val allDeleted = pruneSelectedMessageIds(partial.selectedIds, setOf(19L))
        assertEquals(emptySet<Long>(), allDeleted.selectedIds)
        assertEquals(true, allDeleted.shouldExitSelectionMode)
    }

    @Test
    fun selectionToolbarEnablesOnlyActionsBackedBySelectionAndCapabilities() {
        val outgoing = message("17").copy(text = "hello", isOutgoing = true)
        val incoming = message("18").copy(text = "reply", isOutgoing = false)

        val actions = messageSelectionToolbarActions(
            selectedMessageCount = 2,
            selectedMessages = listOf(outgoing, incoming),
            canForwardMessages = true,
            canDeleteMessages = false,
            deletingMessages = false,
        )

        assertEquals(MessageSelectionToolbarActions(canForward = true, canDelete = false, canCopy = true), actions)
    }

    @Test
    fun selectionToolbarDoesNotExposeActionsForIncompleteOrBusySelection() {
        val message = message("17").copy(text = "hello", isOutgoing = true)

        val incomplete = messageSelectionToolbarActions(
            selectedMessageCount = 2,
            selectedMessages = listOf(message),
            canForwardMessages = true,
            canDeleteMessages = true,
            deletingMessages = false,
        )
        assertEquals(MessageSelectionToolbarActions(false, false, false), incomplete)

        val deleting = messageSelectionToolbarActions(
            selectedMessageCount = 1,
            selectedMessages = listOf(message),
            canForwardMessages = false,
            canDeleteMessages = null,
            deletingMessages = true,
        )
        assertEquals(MessageSelectionToolbarActions(false, false, true), deleting)
    }

    private fun message(id: String) = ConversationMessageUi(
        id = id,
        chatId = "1",
        text = "",
        timestamp = "",
        isOutgoing = false,
    )
}
