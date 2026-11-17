package com.chitik.telegram.notifications

/** Stable Android notification group key shared by all Telegram chat notifications. */
internal object TelegramMessageNotificationGrouping {
    private const val GROUP_KEY = "telegram_message_chats"

    fun groupKeyForChat(chatId: Long): String? = GROUP_KEY.takeIf { chatId != 0L }
}
