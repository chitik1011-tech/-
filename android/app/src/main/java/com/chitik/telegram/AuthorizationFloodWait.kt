package com.chitik.telegram

private val FLOOD_WAIT_MESSAGE = Regex(
    "\\bFLOOD_WAIT:\\s*retry after\\s+(\\d+)\\s+seconds?\\b",
    RegexOption.IGNORE_CASE,
)

/** Extract the structured wait duration from Rust's CoreError display string. */
internal fun authorizationFloodWaitSeconds(error: String): Int? =
    FLOOD_WAIT_MESSAGE.find(error)
        ?.groupValues
        ?.getOrNull(1)
        ?.toIntOrNull()
        ?.takeIf { it > 0 }

internal fun authorizationFloodWaitMessage(seconds: Int): String =
    "Telegram asked you to wait $seconds ${if (seconds == 1) "second" else "seconds"} before trying again."
