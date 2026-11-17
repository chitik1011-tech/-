package com.chitik.telegram.notifications

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TelegramNotificationTagTest {
    @Test
    fun recognizesOnlyExactNonzeroChatTagsOwnedByThePublisher() {
        assertTrue(isTelegramMessageNotificationTag("telegram_chat_42"))
        assertTrue(isTelegramMessageNotificationTag("telegram_chat_-42"))
        assertTrue(isTelegramMessageNotificationTag("telegram_chat_-1000000000042"))

        assertFalse(isTelegramMessageNotificationTag(null))
        assertFalse(isTelegramMessageNotificationTag("other_notification_42"))
        assertFalse(isTelegramMessageNotificationTag("telegram_chat_"))
        assertFalse(isTelegramMessageNotificationTag("telegram_chat_0"))
        assertFalse(isTelegramMessageNotificationTag("telegram_chat_+42"))
        assertFalse(isTelegramMessageNotificationTag("telegram_chat_42_extra"))
    }
}
