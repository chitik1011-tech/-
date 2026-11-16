package com.chitik.telegram

/** UI enables editing only when there is text to place in the edit composer. Telegram RPC remains authoritative. */
internal fun canEditMessageText(message: ConversationMessageUi): Boolean =
    message.text.isNotBlank() && message.poll == null
