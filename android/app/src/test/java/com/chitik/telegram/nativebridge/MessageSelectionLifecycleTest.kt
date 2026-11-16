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
    @Test
    fun ordinaryConnectionLifecycleEventsDoNotClearSelection() {
        val initial = NativeUiState(
            selectionGeneration = 7,
            lastConnectionSessionGeneration = 12,
        )
        val reconnecting = initial.recordConnectionSessionGeneration(12)
        val ready = reconnecting.recordConnectionSessionGeneration(12)

        assertEquals(7L, ready.selectionGeneration)
        assertEquals(12L, ready.lastConnectionSessionGeneration)
    }

    @Test
    fun onlyChangedSessionGenerationClearsSelection() {
        val current = NativeUiState(
            selectionGeneration = 7,
            lastConnectionSessionGeneration = 12,
        )

        val replaced = current.recordConnectionSessionGeneration(13)

        assertEquals(8L, replaced.selectionGeneration)
        assertEquals(13L, replaced.lastConnectionSessionGeneration)
    }

    @Test
    fun lifecyclePayloadReadsSessionGenerationFromToken() {
        val payload = """{"type":"connection_lifecycle","payload":{"token":{"generation":12,"epoch":4},"event":"ready"}}"""

        assertEquals(12L, connectionSessionGenerationFromEvent(payload))
    }

}
