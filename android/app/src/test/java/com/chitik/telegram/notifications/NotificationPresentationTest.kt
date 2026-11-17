package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Test

class NotificationPresentationTest {
    @Test
    fun retainsSenderOrChatWithoutExposingMessageContents() {
        assertEquals(
            NotificationPresentation(title = "Study Group", body = "New message"),
            notificationPresentation(title = "Study Group", body = "Private message text"),
        )
    }

    @Test
    fun fallsBackToGenericTitleAndBodyWhenDetailsAreBlank() {
        assertEquals(
            NotificationPresentation(title = "Telegram", body = "New message"),
            notificationPresentation(title = "", body = ""),
        )
    }
}
