package com.chitik.telegram

/** Stable media viewer identity scoped to a chat, since message IDs are chat-local. */
internal fun mediaViewerKey(chatId: String, messageId: String, attachmentIndex: Int): String =
    "$chatId:$messageId:$attachmentIndex"

internal fun resolveMediaViewerSelection(
    chatId: String,
    messages: List<ConversationMessageUi>,
    viewerKey: String?,
): Pair<Long, MediaAttachmentUi>? {
    val parts = viewerKey?.split(':') ?: return null
    if (parts.size != 3 || parts[0] != chatId) return null
    val messageId = parts[1]
    val attachmentIndex = parts[2].toIntOrNull() ?: return null
    return messages
        .firstOrNull { it.id == messageId }
        ?.attachments
        ?.firstOrNull { it.index == attachmentIndex }
        ?.let { attachment -> messageId.toLongOrNull()?.let { it to attachment } }
}
