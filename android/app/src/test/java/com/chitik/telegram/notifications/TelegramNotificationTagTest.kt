package com.chitik.telegram.notifications

import org.junit.Assert.assertFalse
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TelegramNotificationTagTest {
    @Test
    fun recognizesOnlyExactNonzeroChatTagsOwnedByThePublisher() {
        assertTrue(isTelegramMessageNotificationTag("telegram_account_123_chat_42"))
        assertTrue(isTelegramMessageNotificationTag("telegram_account_123_chat_-42"))
        assertEquals(
            NotificationAccountChatScope(accountUserId = 123L, chatId = -42L),
            telegramMessageNotificationScope("telegram_account_123_chat_-42"),
        )
        assertTrue(isTelegramMessageNotificationTag("telegram_chat_42"))
        assertTrue(isTelegramMessageNotificationTag("telegram_chat_-42"))
        assertTrue(isTelegramMessageNotificationTag("telegram_chat_-1000000000042"))

        assertFalse(isTelegramMessageNotificationTag(null))
        assertFalse(isTelegramMessageNotificationTag("other_notification_42"))
        assertFalse(isTelegramMessageNotificationTag("telegram_chat_"))
        assertFalse(isTelegramMessageNotificationTag("telegram_chat_0"))
        assertFalse(isTelegramMessageNotificationTag("telegram_chat_+42"))
        assertFalse(isTelegramMessageNotificationTag("telegram_chat_42_extra"))
        assertFalse(isTelegramMessageNotificationTag("telegram_account_0_chat_42"))
        assertFalse(isTelegramMessageNotificationTag("telegram_account_123_chat_0"))
        assertFalse(isTelegramMessageNotificationTag("telegram_account_123_chat_+42"))
        assertFalse(isTelegramMessageNotificationTag("telegram_account_123_chat_42_extra"))
    }
}
