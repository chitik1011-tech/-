package com.chitik.telegram.notifications

/**
 * Chooses the pending notification route to restore after Activity recreation.
 *
 * A fresh valid notification tap takes precedence. A malformed or absent fresh
 * route leaves a previously saved valid route intact. Routes are currently
 * chat-ID-only and are not scoped to a Telegram account.
 */
internal fun restorePendingNotificationChatId(
    savedChatId: Long?,
    freshIntentChatId: Long?,
): Long? = freshIntentChatId?.takeIf { it != 0L }
    ?: savedChatId?.takeIf { it != 0L }
