package com.chitik.telegram.notifications

internal data class NotificationPresentation(
    val title: String,
    val body: String,
)

/** Redacts message text while preserving chat or sender context in the notification shade. */
@Suppress("UNUSED_PARAMETER")
internal fun notificationPresentation(title: String, body: String): NotificationPresentation =
    NotificationPresentation(
        title = title.ifBlank { "Telegram" },
        body = "New message",
    )
