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

/** Whether a pending route may remain after the native session restore finishes. */
internal enum class PendingNotificationSessionDecision {
    WaitForRestore,
    KeepPending,
    Discard,
}

/**
 * Retains a route through incomplete restoration and only keeps it after Ready.
 *
 * Routes contain no account ID, so Ready cannot prove that an already-authorized
 * replacement is the same account as the one that created the notification.
 */
internal fun decidePendingNotificationSessionRestore(
    restoreComplete: Boolean,
    authorizationReady: Boolean,
): PendingNotificationSessionDecision = when {
    !restoreComplete -> PendingNotificationSessionDecision.WaitForRestore
    authorizationReady -> PendingNotificationSessionDecision.KeepPending
    else -> PendingNotificationSessionDecision.Discard
}
