package com.chitik.telegram.nativebridge

import com.chitik.telegram.AuthorizationUiStep
import com.chitik.telegram.ConversationMessageUi
import com.chitik.telegram.ConversationReactionUi
import com.chitik.telegram.ReactionRevisionUi
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MessageReactionLifecycleTest {
    @Test
    fun connectionChangeClearsPendingOperationAndKeepsRetryableError() {
        val state = NativeUiState(
            pendingReactionKeys = setOf("-100:12"),
        )

        val updated = state.clearPendingReactionsAfterConnectionChange()

        assertEquals(setOf("8:12"), updated.pendingReactionKeys)
        assertEquals(
            "Результат реакции не подтверждён после смены соединения",
            updated.reactionErrors["-100:12"],
        )
    }

    @Test
    fun syncCompletedDoesNotClearPendingReaction() {
        val state = NativeUiState(
            pendingReactionKeys = setOf("5:12"),
            reactionConnectionToken = 4L to 2L,
        )

        val updated = state.applyConnectionLifecycle(
            """{"payload":{"token":{"generation":4,"epoch":2},"event":{"type":"sync_completed"}}}""",
        )

        assertEquals(setOf("5:12"), updated.pendingReactionKeys)
        assertTrue(updated.reactionErrors.isEmpty())
    }

    @Test
    fun deletionRemovesMessageAndItsPendingAndErrorState() {
        val state = NativeUiState(
            authorization = AuthorizationUiStep.Ready,
            messages = mapOf(
                5L to listOf(
                    message(id = 12L, chatId = 5L),
                    message(id = 13L, chatId = 5L),
                ),
                8L to listOf(message(id = 12L, chatId = 8L)),
            ),
            pendingReactionKeys = setOf("5:12", "8:12"),
            reactionErrors = mapOf("5:13" to "rpc failed"),
        )

        val updated = state.removeDeletedMessages(chatId = 5L, messageIds = setOf(12L, 13L))

        assertEquals(listOf("12"), updated.messages[8L]?.map { it.id })
        assertTrue(updated.messages[5L].isNullOrEmpty())
        assertTrue(updated.pendingReactionKeys.isEmpty())
        assertTrue(updated.reactionErrors.isEmpty())
    }

    @Test
    fun staleReactionSnapshotCannotReplaceNewerVisibleSnapshot() {
        val authoritative = listOf(ConversationReactionUi("emoji", "👍", 4, true))
        val state = NativeUiState(
            messages = mapOf(5L to listOf(message(12L, 5L).copy(
                reactions = authoritative,
                reactionRevision = ReactionRevisionUi(3, 2, 9),
            ))),
            pendingReactionKeys = setOf("5:12"),
        )

        val updated = state.applyReactionUpdate(
            chatId = 5L,
            messageId = 12L,
            reactions = listOf(ConversationReactionUi("emoji", "❤️", 1, false)),
            revision = ReactionRevisionUi(3, 2, 8),
        )

        assertEquals(authoritative, updated.messages[5L]?.single()?.reactions)
        assertEquals(ReactionRevisionUi(3, 2, 9), updated.messages[5L]?.single()?.reactionRevision)
        assertTrue("stale updates must not clear an in-flight operation", "5:12" in updated.pendingReactionKeys)
    }

    private fun message(id: Long, chatId: Long) = ConversationMessageUi(
        id = id.toString(),
        chatId = chatId.toString(),
        text = "",
        timestamp = "",
        isOutgoing = false,
    )
}
