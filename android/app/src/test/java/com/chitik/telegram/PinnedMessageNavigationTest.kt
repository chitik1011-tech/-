package com.chitik.telegram

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import com.chitik.telegram.nativebridge.shouldClearPinnedFailure
import com.chitik.telegram.nativebridge.shouldResolvePinnedAction

class PinnedMessageNavigationTest {
    @Test
    fun pinFailureClearsOnlyWhenItsChatAndMessageChange() {
        assertFalse(shouldClearPinnedFailure("9:7", 9, listOf(3), null))
        assertFalse(shouldClearPinnedFailure("9:7", 10, listOf(7), null))
        assertTrue(shouldClearPinnedFailure("9:7", 9, listOf(7, 8), null))
        assertFalse(shouldClearPinnedFailure("9:7", 9, listOf(7), 42L))
        assertFalse(shouldResolvePinnedAction(42L, 9, listOf(7)))
        assertTrue(shouldResolvePinnedAction(null, 9, listOf(7)))
    }

    @Test
    fun fallbackAppearsOnlyAfterTheTargetPageCompletesWithoutTheMessage() {
        assertFalse(shouldShowPinnedTargetFallback("42", emptyList(), 43L, null))
        assertFalse(shouldShowPinnedTargetFallback("42", emptyList(), null, null))
        assertTrue(shouldShowPinnedTargetFallback("42", emptyList(), null, 43L))
        assertFalse(shouldShowPinnedTargetFallback("42", listOf("42"), null, 43L))
        assertFalse(shouldShowPinnedTargetFallback("bad-id", emptyList(), null, 0L))
    }
}
