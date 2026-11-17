package com.chitik.telegram

/** Only starts recording after permission returns for the still-selected chat. */
internal fun voiceRecordingChatAfterPermission(
    granted: Boolean,
    requestedChatId: Long,
    selectedChatId: Long,
): Long? = requestedChatId.takeIf { granted && it != 0L && it == selectedChatId }

/** Invalidates an outstanding permission request as soon as navigation leaves its chat. */
internal fun pendingVoicePermissionChatAfterNavigation(
    pendingChatId: Long,
    selectedChatId: Long,
): Long = pendingChatId.takeIf { it != 0L && it == selectedChatId } ?: 0L

/** A recording belongs to one chat and must stop before the UI shows a different one. */
internal fun shouldCancelVoiceRecordingForChatChange(
    recordingChatId: Long?,
    selectedChatId: Long,
): Boolean = recordingChatId != null && recordingChatId != selectedChatId
