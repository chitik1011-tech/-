package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class UnreadCounterBadgeTest {
    @Test
    fun formatsSupportedUnreadRangeAndCompactOverflow() {
        assertNull(formatUnreadCount(0))
        assertNull(formatUnreadCount(-1))
        assertEquals("1", formatUnreadCount(1))
        assertEquals("999", formatUnreadCount(999))
        assertEquals("999+", formatUnreadCount(1_000))
        assertEquals("999+", formatUnreadCount(Long.MAX_VALUE))
    }
}
