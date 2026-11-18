package com.chitik.telegram

/** Labels a tappable inline image with its action and filename when Telegram provides one. */
internal fun mediaAttachmentOpenAccessibilityLabel(attachment: MediaAttachmentUi): String {
    val action = if (attachment.kind == MediaKindUi.Photo) "Открыть фото" else "Открыть изображение"
    val filename = attachment.filename
        ?.takeIf(String::isNotBlank)
        ?.let(::sanitizeDisplayFilename)
        ?: return action
    return "$action $filename"
}
