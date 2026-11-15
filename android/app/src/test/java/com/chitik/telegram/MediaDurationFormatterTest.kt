package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaDurationFormatterTest {
    @Test
    fun formatsSubminuteMinuteAndHourBoundaries() {
        assertEquals("0:00", MediaDurationFormatter.format(0))
        assertEquals("0:00", MediaDurationFormatter.format(999))
        assertEquals("0:01", MediaDurationFormatter.format(1_000))
        assertEquals("0:59", MediaDurationFormatter.format(59_999))
        assertEquals("1:00", MediaDurationFormatter.format(60_000))
        assertEquals("59:59", MediaDurationFormatter.format(3_599_999))
        assertEquals("1:00:00", MediaDurationFormatter.format(3_600_000))
        assertEquals("1:01:01", MediaDurationFormatter.format(3_661_000))
    }

    @Test
    fun malformedDurationHasNoLabelAndLargeDurationDoesNotOverflow() {
        assertEquals("", MediaDurationFormatter.format(null))
        assertEquals("", MediaDurationFormatter.format(-1))

        val formatted = MediaDurationFormatter.format(Long.MAX_VALUE)
        val parts = formatted.split(":")
        assertEquals(3, parts.size)
        assertTrue(parts.all { it.isNotEmpty() })
        assertTrue(parts.drop(1).all { it.length == 2 })
    }
}
