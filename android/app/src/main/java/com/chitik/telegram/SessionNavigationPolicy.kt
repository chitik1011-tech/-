package com.chitik.telegram

/**
 * Clears navigation when an active session loses authorization. The initial non-ready value
 * during process restoration is not a transition and must preserve process-restored UI.
 */
internal fun shouldResetNavigationAfterAuthorizationChange(
    wasAuthorizationReady: Boolean,
    isAuthorizationReady: Boolean,
): Boolean = wasAuthorizationReady && !isAuthorizationReady
