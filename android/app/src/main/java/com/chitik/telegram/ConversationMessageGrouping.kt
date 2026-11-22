package com.chitik.telegram

/** Whether a message shares a visual bubble group with the messages around it. */
internal data class ConversationMessageGroupPosition(
    val firstInGroup: Boolean,
    val lastInGroup: Boolean,
)

/** Returns the first loaded incoming message after Telegram's inbox read boundary. */
internal fun firstUnreadIncomingMessageIndex(messages: List<ConversationMessageUi>): Int =
    messages.indexOfFirst { it.isUnread && !it.isOutgoing }

/** A zero or absent boundary is unknown when the snapshot only contains an outbox receipt. */
internal fun isUnreadIncomingMessage(
    messageId: Long,
    isOutgoing: Boolean,
    inboxReadBoundary: Long?,
): Boolean = !isOutgoing && inboxReadBoundary != null && inboxReadBoundary > 0L && messageId > inboxReadBoundary

/**
 * Keep adjacent messages from the same sender on the same day visually grouped.
 * Service messages, sender changes, direction changes, date changes, and unread boundaries break a group.
 */
internal fun conversationMessageGroupPosition(
    messages: List<ConversationMessageUi>,
    index: Int,
): ConversationMessageGroupPosition {
    require(index in messages.indices) { "Message index is outside the conversation" }
    val current = messages[index]
    val previous = messages.getOrNull(index - 1)
    val next = messages.getOrNull(index + 1)

    return ConversationMessageGroupPosition(
        firstInGroup = previous == null || !canShareMessageGroup(previous, current),
        lastInGroup = next == null || !canShareMessageGroup(current, next),
    )
}

private fun canShareMessageGroup(
    first: ConversationMessageUi,
    second: ConversationMessageUi,
): Boolean = first.serviceAction == null &&
    second.serviceAction == null &&
    first.chatId == second.chatId &&
    first.isUnread == second.isUnread &&
    first.dateSeparator.isNotBlank() &&
    first.dateSeparator == second.dateSeparator &&
    first.isOutgoing == second.isOutgoing &&
    (first.isOutgoing || (first.senderId != 0L && first.senderId == second.senderId))
