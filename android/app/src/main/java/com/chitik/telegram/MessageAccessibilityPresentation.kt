package com.chitik.telegram

/** Accessible summary built only from fields already rendered for a message. */
internal fun messageBubbleAccessibilityLabel(message: ConversationMessageUi): String = buildList {
    add(if (message.isOutgoing) "Ваше сообщение" else "Входящее сообщение")
    message.forwardedFrom?.takeIf(String::isNotBlank)?.let { add("Переслано от $it") }
    message.text.trim().takeIf(String::isNotEmpty)?.let { add(it) }
    message.attachments.forEach { attachment ->
        val kind = when (attachment.kind) {
            MediaKindUi.Photo -> "Фото"
            MediaKindUi.Video -> "Видео"
            MediaKindUi.Document -> attachment.filename
                ?.takeIf(String::isNotBlank)
                ?.let(::sanitizeDisplayFilename)
                ?.let { "Файл $it" }
                ?: "Файл"
            MediaKindUi.Audio -> "Аудио"
            MediaKindUi.Voice -> "Голосовое сообщение"
            MediaKindUi.Animation -> "Анимация"
            MediaKindUi.Sticker -> "Стикер"
            MediaKindUi.VideoNote -> "Видеосообщение"
            MediaKindUi.Unknown -> "Вложение"
        }
        add(kind)
    }
    message.poll?.question?.trim()?.takeIf(String::isNotEmpty)?.let { add("Опрос: $it") }
    message.location?.let { location ->
        add(when (location) {
            is ConversationLocationUi.Venue -> location.title.takeIf(String::isNotBlank) ?: "Место"
            is ConversationLocationUi.Point -> "Местоположение"
        })
    }
    if (message.timestamp.isNotBlank()) add("Время: ${message.timestamp}")
    if (message.timestamp.isNotBlank() && message.isOutgoing && message.isRead) add("Прочитано")
}.joinToString(". ")

internal fun messageContextActionLabel(selectionMode: Boolean): String =
    if (selectionMode) "Изменить выбор сообщения" else "Открыть действия сообщения"

internal fun messageClickAccessibilityLabel(selectionMode: Boolean): String? =
    if (selectionMode) "Выбрать или снять выбор сообщения" else null
