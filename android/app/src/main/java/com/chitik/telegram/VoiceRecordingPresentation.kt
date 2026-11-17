package com.chitik.telegram

/** Keeps a recording timer readable without ever showing a negative elapsed value. */
internal fun formatVoiceRecordingDuration(durationMillis: Long): String =
    MediaDurationFormatter.format(durationMillis.coerceAtLeast(0L))
