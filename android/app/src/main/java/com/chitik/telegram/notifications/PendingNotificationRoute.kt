package com.chitik.telegram.notifications

/**
 * Chooses the pending notification route to restore after Activity recreation.
 * A fresh valid tap takes precedence. Saved routes are reconstructed only when both
 * their chat and Telegram account identifiers are valid.
 */
internal fun restorePendingNotificationRoute(
    savedChatId: Long?,
    savedAccountUserId: Long?,
    freshIntentRoute: TelegramNotificationRoute?,
): TelegramNotificationRoute? = freshIntentRoute ?: runCatching {
    if (savedChatId == null || savedAccountUserId == null) return@runCatching null
    TelegramNotificationRoute(savedChatId, savedAccountUserId)
}.getOrNull()

/** Whether a pending route may remain after the native session restore finishes. */
internal enum class PendingNotificationSessionDecision {
    WaitForRestore,
    KeepPending,
    Discard,
}

/**
 * Retains a route through incomplete restoration and only keeps it after Ready.
 * The route gate separately requires the resolved Telegram account ID to match.
 */
internal fun decidePendingNotificationSessionRestore(
    restoreComplete: Boolean,
    authorizationReady: Boolean,
): PendingNotificationSessionDecision = when {
    !restoreComplete -> PendingNotificationSessionDecision.WaitForRestore
    authorizationReady -> PendingNotificationSessionDecision.KeepPending
    else -> PendingNotificationSessionDecision.Discard
}
