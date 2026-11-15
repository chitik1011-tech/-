package com.chitik.telegram

/** Formats media durations stored in milliseconds for compact player UI. */
object MediaDurationFormatter {
    /** Returns `m:ss` or `h:mm:ss`; null and negative durations have no label. */
    fun format(durationMillis: Long?): String {
        val totalSeconds = durationMillis?.takeIf { it >= 0L }?.div(1_000L) ?: return ""
        val hours = totalSeconds / 3_600L
        val minutes = (totalSeconds % 3_600L) / 60L
        val seconds = totalSeconds % 60L

        return if (hours > 0L) {
            "${hours}:${twoDigits(minutes)}:${twoDigits(seconds)}"
        } else {
            "${totalSeconds / 60L}:${twoDigits(seconds)}"
        }
    }

    private fun twoDigits(value: Long): String = if (value < 10L) "0$value" else value.toString()
}
