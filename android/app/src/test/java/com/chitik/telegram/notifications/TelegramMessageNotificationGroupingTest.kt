package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class TelegramMessageNotificationGroupingTest {
    @Test
    fun allValidChatsShareOneNotificationGroup() {
        val groupKey = TelegramMessageNotificationGrouping.groupKeyForChat(123L)

        assertEquals("telegram_message_chats", groupKey)
        assertEquals(groupKey, TelegramMessageNotificationGrouping.groupKeyForChat(-456L))
    }

    @Test
    fun invalidChatDoesNotReceiveNotificationGroup() {
        assertNull(TelegramMessageNotificationGrouping.groupKeyForChat(0L))
    }

    @Test
    fun summaryAppearsOnlyWhileChildrenExistAndNeverIncludesMessageText() {
        assertNull(TelegramMessageNotificationGrouping.summaryContent(0))
        assertNull(TelegramMessageNotificationGrouping.summaryContent(-1))
        assertEquals(
            TelegramMessageNotificationGrouping.SummaryContent(
                title = "Telegram",
                body = "New messages",
            ),
            TelegramMessageNotificationGrouping.summaryContent(2),
        )
    }

    @Test
    fun summaryTagIsNotCountedAsAChatNotification() {
        assertFalse(
            isTelegramMessageNotificationTag(TelegramMessageNotificationGrouping.SUMMARY_TAG),
        )
    }
}
