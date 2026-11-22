package com.chitik.telegram

internal fun pinnedMessageBannerLabel(
    messageId: String,
    messages: List<ConversationMessageUi>,
    pinnedCount: Int,
): String {
    val message = messages.lastOrNull { it.id == messageId }
    val preview = message?.let(::pinnedMessagePreview) ?: "Закреплённое сообщение"
    val count = pinnedCount.coerceAtLeast(1)
    val prefix = if (count > 1) "Закреплено ($count)" else "Закреплено"
    return "$prefix: $preview"
}

private fun pinnedMessagePreview(message: ConversationMessageUi): String =
    message.text.trim().takeIf(String::isNotEmpty)
        ?: message.poll?.question?.trim()?.takeIf(String::isNotEmpty)?.let { "Опрос: $it" }
        ?: message.attachments.firstOrNull()?.let { attachment ->
            when (attachment.kind) {
                MediaKindUi.Photo -> "Фото"
                MediaKindUi.Video -> "Видео"
                MediaKindUi.Document -> attachment.filename?.takeIf(String::isNotBlank)?.let { "Файл: $it" } ?: "Файл"
                MediaKindUi.Audio -> "Аудио"
                MediaKindUi.Voice -> "Голосовое сообщение"
                MediaKindUi.Animation -> "Анимация"
                MediaKindUi.Sticker -> "Стикер"
                MediaKindUi.VideoNote -> "Видеосообщение"
                MediaKindUi.Unknown -> "Вложение"
            }
        }
        ?: message.location?.let { location ->
            when (location) {
                is ConversationLocationUi.Venue -> location.title.takeIf(String::isNotBlank) ?: "Место"
                is ConversationLocationUi.Point -> "Местоположение"
            }
        }
        ?: "Сообщение"
