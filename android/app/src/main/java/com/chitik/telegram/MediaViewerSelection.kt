package com.chitik.telegram

/** Stable media viewer identity scoped to a chat, since message IDs are chat-local. */
internal fun mediaViewerKey(chatId: String, messageId: String, attachmentIndex: Int): String =
    "$chatId:$messageId:$attachmentIndex"

internal data class MediaViewerItem(
    val messageId: Long,
    val attachment: MediaAttachmentUi,
)

internal data class MediaViewerSelection(
    val items: List<MediaViewerItem>,
    val index: Int,
) {
    val current: MediaViewerItem get() = items[index]
    val previous: MediaViewerItem? get() = items.getOrNull(index - 1)
    val next: MediaViewerItem? get() = items.getOrNull(index + 1)
}

/**
 * Resolve the opened item and its neighbors from messages already loaded for this chat.
 * The viewer does not start downloads; uncached neighbors use the existing retry control.
 */
internal fun resolveMediaViewerSelection(
    chatId: String,
    messages: List<ConversationMessageUi>,
    viewerKey: String?,
): MediaViewerSelection? {
    val parts = viewerKey?.split(':') ?: return null
    if (parts.size != 3 || parts[0] != chatId) return null
    val messageId = parts[1].toLongOrNull() ?: return null
    val attachmentIndex = parts[2].toIntOrNull() ?: return null

    val items = messages.asSequence()
        .filter { it.chatId == chatId }
        .flatMap { message ->
            val id = message.id.toLongOrNull() ?: return@flatMap emptySequence()
            message.attachments.asSequence()
                .filter { it.index >= 0 && isMediaViewerAttachment(it) }
                .sortedBy { it.index }
                .map { attachment -> MediaViewerItem(id, attachment) }
        }
        .toList()
    val index = items.indexOfFirst {
        it.messageId == messageId && it.attachment.index == attachmentIndex
    }
    return index.takeIf { it >= 0 }?.let { MediaViewerSelection(items, it) }
}

private fun isMediaViewerAttachment(attachment: MediaAttachmentUi): Boolean =
    isInlineImageMedia(attachment) ||
        attachment.kind == MediaKindUi.Video ||
        attachment.kind == MediaKindUi.VideoNote
