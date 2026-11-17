package com.chitik.telegram.notifications

/** Result of resolving a notification tap against the currently restored Telegram account. */
internal sealed interface NotificationRouteResolution {
    data object Wait : NotificationRouteResolution
    data class OpenChat(val chatId: Long) : NotificationRouteResolution
    data object Discard : NotificationRouteResolution
}

/**
 * Keeps a valid tap pending until authorization and at least an initial account chat snapshot
 * are available. A missing target is discarded only when pagination confirms the list is complete.
 * [chatsLoaded] means an initial snapshot exists, not that all pages have been read. Until the
 * caller passes [chatListExhausted] from the pagination result, a missing target remains pending.
 * @param chatListExhausted true only after the backend confirms that no further pages exist.
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
