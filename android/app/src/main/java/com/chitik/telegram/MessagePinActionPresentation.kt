package com.chitik.telegram

internal data class MessagePinActionPresentation(
    val label: String?,
    val enabled: Boolean,
    val pending: Boolean,
)

/** Pinning belongs to the long-press action menu, and is limited to server messages. */
internal fun messagePinActionPresentation(
    message: ConversationMessageUi,
    canPin: Boolean,
    isPinned: Boolean,
    isPending: Boolean,
    mutationPending: Boolean,
): MessagePinActionPresentation {
    val eligible = message.id.toLongOrNull()?.let { it > 0L } == true && message.serviceAction == null
    val label = if (canPin && eligible) {
        if (isPinned) "Открепить" else "Закрепить"
    } else {
        null
    }
    return MessagePinActionPresentation(
        label = label,
        enabled = label != null && !isPending && !mutationPending,
        pending = isPending,
    )
}
