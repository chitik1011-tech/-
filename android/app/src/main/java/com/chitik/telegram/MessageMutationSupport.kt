package com.chitik.telegram

/** UI enables editing only when there is text to place in the edit composer. Telegram RPC remains authoritative. */
internal fun canEditMessageText(message: ConversationMessageUi): Boolean =
    message.text.isNotBlank() && message.poll == null

/** Only a command-specific server result for this exact message may close its action UI. */
internal fun matchesMutationTarget(
    pendingChatId: Long,
    pendingMessageId: Long,
    resultChatId: Long,
    resultMessageId: Long,
): Boolean =
    pendingChatId != 0L &&
        pendingMessageId > 0L &&
        pendingChatId == resultChatId &&
        pendingMessageId == resultMessageId

/** Telegram update notifications can be remote or unrelated; only a command result is an ack. */
internal fun mutationEventAcknowledges(
    eventType: String,
    pendingChatId: Long,
    pendingMessageId: Long,
    resultChatId: Long,
    resultMessageId: Long,
): Boolean =
    eventType == "message_mutation_succeeded" &&
        matchesMutationTarget(pendingChatId, pendingMessageId, resultChatId, resultMessageId)

internal data class MessageMutationRequestState(
    val pending: Boolean,
    val error: String?,
)

internal fun messageMutationStateAfterSessionChange(
    pending: Boolean,
    currentError: String?,
    reason: String,
): MessageMutationRequestState =
    if (pending) {
        MessageMutationRequestState(pending = false, error = reason)
    } else {
        MessageMutationRequestState(pending = false, error = currentError)
    }
