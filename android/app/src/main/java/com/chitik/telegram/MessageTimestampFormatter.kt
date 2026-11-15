package com.chitik.telegram

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/** Formats Telegram Unix-second timestamps using the user's current timezone. */
object MessageTimestampFormatter {
    /**
     * Shows a local time for today's messages, a local date for this year, and
     * a local date with year for older messages. Optional inputs make formatting
     * deterministic for callers that need tests or a fixed conversation time.
     */
    fun format(
        timestampUnixSeconds: Long,
        now: Instant = Instant.now(),
        zoneId: ZoneId = ZoneId.systemDefault(),
        locale: Locale = Locale.getDefault(),
    ): String = runCatching {
        val timestamp = Instant.ofEpochSecond(timestampUnixSeconds).atZone(zoneId)
        val today = now.atZone(zoneId).toLocalDate()
        val messageDate = timestamp.toLocalDate()

        when {
            messageDate == today -> DateTimeFormatter.ofPattern("HH:mm", locale).format(timestamp)
            messageDate.year == today.year -> DateTimeFormatter.ofPattern("d MMM", locale).format(timestamp)
            else -> DateTimeFormatter.ofPattern("d MMM yyyy", locale).format(timestamp)
        }
    }.getOrDefault("")
}
