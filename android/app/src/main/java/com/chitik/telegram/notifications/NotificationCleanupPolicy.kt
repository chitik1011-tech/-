package com.chitik.telegram.notifications

/** Keeps posted-message cleanup tied to confirmed account transitions. */
internal object NotificationCleanupPolicy {
    fun shouldClearOnAuthorizationChange(
        wasAuthorized: Boolean,
        authorizationEventParsed: Boolean,
        isAuthorizedNow: Boolean,
    ): Boolean = wasAuthorized && authorizationEventParsed && !isAuthorizedNow

    fun shouldClearOnRestoredAuthorizationState(
        restoreWasComplete: Boolean,
        restoreIsComplete: Boolean,
        isAuthorized: Boolean,
    ): Boolean = !restoreWasComplete && restoreIsComplete && !isAuthorized

}
