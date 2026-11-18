package com.chitik.telegram

/** Whether a message shares a visual bubble group with the messages around it. */
internal data class ConversationMessageGroupPosition(
    val firstInGroup: Boolean,
    val lastInGroup: Boolean,
)

/**
 * Keep adjacent messages from the same sender on the same day visually grouped.
 * Service messages, sender changes, direction changes, and date boundaries break a group.
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
    first.dateSeparator.isNotBlank() &&
    first.dateSeparator == second.dateSeparator &&
    first.isOutgoing == second.isOutgoing &&
    (first.isOutgoing || (first.senderId != 0L && first.senderId == second.senderId))
