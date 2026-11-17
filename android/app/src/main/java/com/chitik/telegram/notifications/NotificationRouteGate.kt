package com.chitik.telegram.notifications

/** Result of resolving a notification tap against the currently restored Telegram account. */
internal sealed interface NotificationRouteResolution {
    data object Wait : NotificationRouteResolution
    data class OpenChat(val chatId: Long) : NotificationRouteResolution
    data object Discard : NotificationRouteResolution
}

/**
 * Keeps a valid tap pending until authorization and an initial chat snapshot are ready. A missing
 * target is discarded only after the caller confirms the cursor stream is exhausted.
 */
internal fun resolveNotificationRoute(
    chatId: Long?,
    authorized: Boolean,
    chatsLoaded: Boolean,
    availableChatIds: Collection<Long>,
    chatListExhausted: Boolean = false,
): NotificationRouteResolution {
    val target = chatId?.takeIf { it != 0L } ?: return NotificationRouteResolution.Discard
    if (!authorized || !chatsLoaded) return NotificationRouteResolution.Wait
    if (target in availableChatIds) return NotificationRouteResolution.OpenChat(target)
    return if (chatListExhausted) NotificationRouteResolution.Discard else NotificationRouteResolution.Wait
}
