package com.chitik.telegram.notifications

/** Controls which message details appear in the unlocked notification shade. */
internal enum class NotificationPreviewPolicy {
    Full,
    SenderAndChat,
    Hidden,
}

internal data class NotificationPresentation(
    val title: String,
    val body: String,
)

/** The default avoids exposing message contents while retaining who or which chat sent it. */
internal val DEFAULT_NOTIFICATION_PREVIEW_POLICY = NotificationPreviewPolicy.SenderAndChat

internal fun notificationPresentation(
    policy: NotificationPreviewPolicy,
    title: String,
    body: String,
): NotificationPresentation = when (policy) {
    NotificationPreviewPolicy.Full -> NotificationPresentation(
        title = title.ifBlank { "Telegram" },
        body = body.ifBlank { "New message" },
    )
    NotificationPreviewPolicy.SenderAndChat -> NotificationPresentation(
        title = title.ifBlank { "Telegram" },
        body = "New message",
    )
    NotificationPreviewPolicy.Hidden -> NotificationPresentation(
        title = "Telegram",
        body = "New message",
    )
}
