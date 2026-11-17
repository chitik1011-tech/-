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

    @Test
    fun summaryUsesEnabledActiveChildChannelWhenAvailable() {
        assertEquals(
            "group",
            TelegramMessageNotificationGrouping.summaryChannelId(
                activeChildChannels = listOf("private", "group"),
                enabledChannels = setOf("group"),
                privateChannelId = "private",
                groupChannelId = "group",
            ),
        )
    }

    @Test
    fun summaryFallsBackToEnabledPrivateChannelWhenLastChildChannelIsBlocked() {
        assertEquals(
            "private",
            TelegramMessageNotificationGrouping.summaryChannelId(
                activeChildChannels = listOf("group"),
                enabledChannels = setOf("private"),
                privateChannelId = "private",
                groupChannelId = "group",
            ),
        )
    }

    @Test
    fun summaryHasNoChannelWhenAllChannelsAreBlocked() {
        assertNull(
            TelegramMessageNotificationGrouping.summaryChannelId(
                activeChildChannels = listOf("group", "private"),
                enabledChannels = emptySet(),
                privateChannelId = "private",
                groupChannelId = "group",
            ),
        )
    }
}
