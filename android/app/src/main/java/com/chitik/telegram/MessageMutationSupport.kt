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
