package com.chitik.telegram

/**
 * Clears navigation when an active session loses authorization. The initial non-ready value
 * during process restoration is not a transition and must preserve process-restored UI.
 */
internal fun shouldResetNavigationAfterAuthorizationChange(
    wasAuthorizationReady: Boolean,
    isAuthorizationReady: Boolean,
): Boolean = wasAuthorizationReady && !isAuthorizationReady

/** Clears a restored chat selection once its saved Telegram account no longer matches. */
internal fun shouldClearChatSelectionForAccountMismatch(
    selectedChatId: Long,
    selectedChatAccountUserId: Long?,
    accountIdentityResolved: Boolean,
    activeAccountUserId: Long?,
): Boolean = selectedChatId != 0L && accountIdentityResolved &&
    !chatSelectionBelongsToActiveAccount(selectedChatAccountUserId, accountIdentityResolved, activeAccountUserId)

/** A persisted chat selection is trusted only after matching a resolved positive Telegram ID. */
internal fun chatSelectionBelongsToActiveAccount(
    selectedChatAccountUserId: Long?,
    accountIdentityResolved: Boolean,
    activeAccountUserId: Long?,
): Boolean = accountIdentityResolved && selectedChatAccountUserId != null &&
    selectedChatAccountUserId > 0L && selectedChatAccountUserId == activeAccountUserId
