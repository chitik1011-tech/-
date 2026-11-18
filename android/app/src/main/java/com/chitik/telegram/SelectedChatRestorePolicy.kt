package com.chitik.telegram

/** Reload a restored selected chat only when its durable message cache is absent. */
internal fun shouldLoadRestoredSelectedChat(
    activityStateRestored: Boolean,
    sessionRestoreComplete: Boolean,
    authorizationReady: Boolean,
    selectedChatId: Long,
    hasCachedMessages: Boolean,
    explicitlyLoadedForChat: Boolean,
    hasPendingLink: Boolean,
): Boolean = activityStateRestored &&
    sessionRestoreComplete &&
    authorizationReady &&
    selectedChatId != 0L &&
    !hasCachedMessages &&
    !explicitlyLoadedForChat &&
    !hasPendingLink
