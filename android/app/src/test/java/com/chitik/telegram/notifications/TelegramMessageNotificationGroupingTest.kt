package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
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
}
