package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationCancellationPolicyTest {
    @Test
    fun readReceiptCancelsOnlyAtOrBelowInboxBoundary() {
        assertTrue(NotificationCancellationPolicy.isReadByBoundary(10L, 10L))
        assertTrue(NotificationCancellationPolicy.isReadByBoundary(8L, 10L))
        assertFalse(NotificationCancellationPolicy.isReadByBoundary(11L, 10L))
        assertFalse(NotificationCancellationPolicy.isReadByBoundary(10L, 0L))
        assertTrue(NotificationCancellationPolicy.isReadByBoundary(null, 10L))
    }

    @Test
    fun readEventsRequireValidChatAndInboxDirection() {
        assertEquals(
            NotificationCancellation.ReadThrough(chatId = -4L, maxMessageId = 30L),
            NotificationCancellationPolicy.fromEvent(
                "read_receipt_updated",
                """{"payload":{"chat_id":-4,"direction":"inbox","max_message_id":30}}""",
            ),
        )
        assertNull(
            NotificationCancellationPolicy.fromEvent(
                "read_receipt_updated",
                """{"payload":{"chat_id":-4,"direction":"outbox","max_message_id":30}}""",
            ),
        )
        assertNull(
            NotificationCancellationPolicy.fromEvent(
                "chat_read",
                """{"payload":{"chat_id":0,"max_message_id":30}}""",
            ),
        )
    }

    @Test
    fun editsAndDeletesMatchOnlyTheAffectedMessageIds() {
        assertEquals(
            NotificationCancellation.MessagesChanged(chatId = 9L, messageIds = setOf(12L)),
            NotificationCancellationPolicy.fromEvent(
                "message_edited",
                """{"payload":{"chat_id":9,"id":12}}""",
            ),
        )
        assertEquals(
            NotificationCancellation.MessagesChanged(chatId = 9L, messageIds = setOf(12L, 14L)),
            NotificationCancellationPolicy.fromEvent(
                "messages_deleted",
                """{"payload":{"chat_id":9,"message_ids":[12,14]}}""",
            ),
        )
        assertTrue(NotificationCancellationPolicy.matchesChangedMessage(12L, setOf(12L, 14L)))
        assertFalse(NotificationCancellationPolicy.matchesChangedMessage(13L, setOf(12L, 14L)))
        assertTrue(NotificationCancellationPolicy.matchesChangedMessage(null, setOf(12L)))
    }

    @Test
    fun malformedOrUnscopedDeletesNeverCancelAcrossChats() {
        assertNull(
            NotificationCancellationPolicy.fromEvent(
                "messages_deleted",
                """{"payload":{"chat_id":null,"message_ids":[12]}}""",
            ),
        )
        assertNull(
            NotificationCancellationPolicy.fromEvent(
                "messages_deleted",
                """{"payload":{"chat_id":9,"message_ids":[]}}""",
            ),
        )
    }

    @Test
    fun notificationTagMustIdentifyOneCanonicalChat() {
        assertEquals(-9L, telegramMessageNotificationChatId("telegram_chat_-9"))
        assertNull(telegramMessageNotificationChatId("telegram_chat_9_suffix"))
        assertNull(telegramMessageNotificationChatId("telegram_message_group_summary"))
    }
}
