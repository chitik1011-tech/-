package com.chitik.telegram

/**
 * Allows bulk deletion of the exact current selection when every message is
 * outgoing, or Telegram explicitly grants this chat delete-message rights.
 * Unknown capability snapshots never authorize deleting incoming messages.
 */
internal fun canBulkDeleteMessages(
    selectedMessageCount: Int,
    selectedMessages: Collection<ConversationMessageUi>,
    canDeleteMessages: Boolean?,
): Boolean {
    if (selectedMessageCount <= 0 || selectedMessages.size != selectedMessageCount) return false
    return selectedMessages.all { it.isOutgoing } || canDeleteMessages == true
}
