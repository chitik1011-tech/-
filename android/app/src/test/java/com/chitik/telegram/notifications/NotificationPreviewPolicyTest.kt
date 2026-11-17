package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Test

class NotificationPreviewPolicyTest {
    @Test
    fun defaultPolicyShowsSenderOrChatWithoutMessageContents() {
        assertEquals(NotificationPreviewPolicy.SenderAndChat, DEFAULT_NOTIFICATION_PREVIEW_POLICY)
        assertEquals(
            NotificationPresentation(title = "Study Group", body = "New message"),
            notificationPresentation(
                DEFAULT_NOTIFICATION_PREVIEW_POLICY,
                title = "Study Group",
                body = "Private message text",
            ),
        )
    }

    @Test
    fun fullPolicyShowsMessageBodyAndUsesFallbackForBlankContent() {
        assertEquals(
            NotificationPresentation(title = "Alice", body = "Hello"),
            notificationPresentation(NotificationPreviewPolicy.Full, "Alice", "Hello"),
        )
        assertEquals(
            NotificationPresentation(title = "Telegram", body = "New message"),
            notificationPresentation(NotificationPreviewPolicy.Full, "", ""),
        )
    }

    @Test
    fun hiddenPolicyHidesChatAndMessageDetails() {
        assertEquals(
            NotificationPresentation(title = "Telegram", body = "New message"),
            notificationPresentation(NotificationPreviewPolicy.Hidden, "Study Group", "Private text"),
        )
    }
}
