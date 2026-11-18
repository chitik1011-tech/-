package com.chitik.telegram

/** Describes only the attachment choices currently wired into the composer. */
internal fun composerAttachmentActionAccessibilityLabel(
    hasPhotoVideoPicker: Boolean,
    hasDocumentPicker: Boolean,
): String = when {
    hasPhotoVideoPicker && hasDocumentPicker -> "Добавить фото, видео или файл"
    hasPhotoVideoPicker -> "Добавить фото или видео"
    hasDocumentPicker -> "Добавить файл"
    else -> "Добавить вложение"
}

internal fun composerLocationActionAccessibilityLabel(): String = "Отправить геопозицию"

internal fun composerSendActionAccessibilityLabel(sending: Boolean): String =
    if (sending) "Отправка сообщения" else "Отправить сообщение"
