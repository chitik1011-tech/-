package com.chitik.telegram.notifications

/** Result of resolving a notification tap against the currently restored Telegram account. */
internal sealed interface NotificationRouteResolution {
    data object Wait : NotificationRouteResolution
    data class OpenChat(val chatId: Long) : NotificationRouteResolution
    data object Discard : NotificationRouteResolution
}

/** Keeps a valid tap pending until authorization and the current account's chat list are ready. */
internal fun resolveNotificationRoute(
    chatId: Long?,
    authorized: Boolean,
    chatsLoaded: Boolean,
    availableChatIds: Collection<Long>,
): NotificationRouteResolution {
    val target = chatId?.takeIf { it != 0L } ?: return NotificationRouteResolution.Discard
    if (!authorized || !chatsLoaded) return NotificationRouteResolution.Wait
    return if (target in availableChatIds) {
        NotificationRouteResolution.OpenChat(target)
    } else {
        NotificationRouteResolution.Discard
    }
}
