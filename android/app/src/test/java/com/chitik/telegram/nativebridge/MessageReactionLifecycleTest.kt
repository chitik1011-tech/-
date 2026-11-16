package com.chitik.telegram.nativebridge

import com.chitik.telegram.AuthorizationUiStep
import com.chitik.telegram.ConversationMessageUi
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

        assertTrue(updated.pendingReactionKeys.isEmpty())
        assertEquals(
            "Результат реакции не подтверждён после смены соединения",
            updated.reactionErrors["-100:12"],
        )
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

    private fun message(id: Long, chatId: Long) = ConversationMessageUi(
        id = id.toString(),
        chatId = chatId.toString(),
        text = "",
        timestamp = "",
        isOutgoing = false,
    )
}
