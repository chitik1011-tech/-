package com.chitik.telegram.notifications

/** Stable Android notification group key shared by all Telegram chat notifications. */
internal object TelegramMessageNotificationGrouping {
    const val GROUP_KEY = "telegram_message_chats"
    const val SUMMARY_TAG = "telegram_message_group_summary"

    data class SummaryContent(
        val title: String,
        val body: String,
    )

    fun groupKeyForChat(chatId: Long): String? = GROUP_KEY.takeIf { chatId != 0L }

    fun summaryContent(chatNotificationCount: Int): SummaryContent? =
        if (chatNotificationCount > 0) {
            SummaryContent(title = "Telegram", body = "New messages")
        } else {
            null
        }
}
