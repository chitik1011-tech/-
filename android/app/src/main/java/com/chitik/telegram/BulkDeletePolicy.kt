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

internal data class MessageSelectionToolbarActions(
    val canForward: Boolean,
    val canDelete: Boolean,
    val canCopy: Boolean,
)

/** Resolves only bulk actions backed by the current selection and live chat capabilities. */
internal fun messageSelectionToolbarActions(
    selectedMessageCount: Int,
    selectedMessages: Collection<ConversationMessageUi>,
    canForwardMessages: Boolean?,
    canDeleteMessages: Boolean?,
    deletingMessages: Boolean,
): MessageSelectionToolbarActions {
    val selectionIsComplete = selectedMessageCount > 0 && selectedMessages.size == selectedMessageCount
    return MessageSelectionToolbarActions(
        canForward = selectionIsComplete && canForwardMessages == true,
        canDelete = selectionIsComplete && !deletingMessages && canBulkDeleteMessages(
            selectedMessageCount = selectedMessageCount,
            selectedMessages = selectedMessages,
            canDeleteMessages = canDeleteMessages,
        ),
        canCopy = selectionIsComplete && selectedMessages.all { it.text.isNotBlank() },
    )
}
