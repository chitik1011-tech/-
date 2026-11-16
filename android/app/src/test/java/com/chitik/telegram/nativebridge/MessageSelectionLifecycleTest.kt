package com.chitik.telegram.nativebridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class MessageSelectionLifecycleTest {
    @Test
    fun generationChangeClearsPendingDeleteAndReportsUnconfirmedResult() {
        val pending = NativeUiState(
            selectionGeneration = 7,
            deletingMessages = true,
        )

        val next = pending.advanceSelectionGeneration()

        assertEquals(8L, next.selectionGeneration)
        assertFalse(next.deletingMessages)
        assertEquals(
            "Удаление прервано; обновите сообщения и проверьте результат",
            next.deleteError,
        )
    }
}
