package com.chitik.telegram

import java.time.Instant
import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MessageTimestampFormatterTest {
    private val locale = Locale.US

    @Test
    fun formatsTimeOnTheSameLocalDayAcrossUtcDateBoundary() {
        val result = MessageTimestampFormatter.format(
            timestampUnixSeconds = Instant.parse("2026-11-14T21:45:00Z").epochSecond,
            now = Instant.parse("2026-11-15T00:30:00Z"),
            zoneId = ZoneId.of("Europe/Moscow"),
            locale = locale,
        )

        assertEquals("00:45", result)
    }

    @Test
    fun formatsDateInsteadOfTimeForEarlierLocalDayInTheSameYear() {
        val result = MessageTimestampFormatter.format(
            timestampUnixSeconds = Instant.parse("2026-11-14T20:45:00Z").epochSecond,
            now = Instant.parse("2026-11-15T00:30:00Z"),
            zoneId = ZoneId.of("Europe/Moscow"),
            locale = locale,
        )

        assertTrue(result.contains("14"))
        assertTrue(result.contains("Nov"))
        assertTrue(!result.contains("2026"))
    }

    @Test
    fun includesYearForPreviousYearAndReturnsEmptyForUnsupportedInstant() {
        val result = MessageTimestampFormatter.format(
            timestampUnixSeconds = Instant.parse("2025-12-31T23:00:00Z").epochSecond,
            now = Instant.parse("2026-01-01T01:00:00Z"),
            zoneId = ZoneId.of("UTC"),
            locale = locale,
        )

        assertTrue(result.contains("2025"))
        assertEquals(
            "",
            MessageTimestampFormatter.format(
                timestampUnixSeconds = Long.MAX_VALUE,
                now = Instant.EPOCH,
                zoneId = ZoneId.of("UTC"),
                locale = locale,
            ),
        )
    }
}
