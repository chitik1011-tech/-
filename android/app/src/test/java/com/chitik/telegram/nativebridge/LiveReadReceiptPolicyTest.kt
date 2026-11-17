package com.chitik.telegram.nativebridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class LiveReadReceiptPolicyTest {
    @Test
    fun onlyVisibleAuthorizedIncomingMessagesAdvanceMonotonically() {
        val policy = LiveReadReceiptPolicy()
        policy.reset(4L)
        policy.updateVisibility(isForeground = true, chatId = -12L)

        assertEquals(20L, policy.onIncomingMessage(4L, -12L, 20L, false, true))
        assertNull(policy.onIncomingMessage(4L, -12L, 18L, false, true))
        assertNull(policy.onIncomingMessage(4L, -12L, 20L, false, true))
        assertEquals(21L, policy.onIncomingMessage(4L, -12L, 21L, false, true))
    }

    @Test
    fun rejectsInvalidContextAndClearsDedupeOnSessionReplacement() {
        val policy = LiveReadReceiptPolicy()
        policy.reset(4L)
        policy.updateVisibility(isForeground = true, chatId = 8L)

        assertNull(policy.onIncomingMessage(3L, 8L, 1L, false, true))
        assertNull(policy.onIncomingMessage(4L, 9L, 1L, false, true))
        assertNull(policy.onIncomingMessage(4L, 8L, 0L, false, true))
        assertNull(policy.onIncomingMessage(4L, 8L, 1L, true, true))
        assertNull(policy.onIncomingMessage(4L, 8L, 1L, false, false))
        assertEquals(1L, policy.onIncomingMessage(4L, 8L, 1L, false, true))
        assertNull(policy.onIncomingMessage(4L, 8L, 1L, false, true))

        policy.reset(5L)
        policy.updateVisibility(isForeground = true, chatId = 8L)
        assertEquals(1L, policy.onIncomingMessage(5L, 8L, 1L, false, true))
    }
}
