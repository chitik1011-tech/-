package com.chitik.telegram

import java.time.Instant
import java.time.ZoneOffset
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ChatListRowPresentationTest {
    private val now = Instant.parse("2025-03-12T18:00:00Z")

    @Test
    fun incomingPreviewUsesActualTextTimeAndUnreadCount() {
        val row = chat(
            unreadCount = 12,
            lastMessage = ChatListPreview("  On my way  ", now.minusSeconds(60).epochSecond, false),
        ).toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)

        assertEquals("Morgan", row.title)
        assertEquals("17:59", row.timestamp)
        assertEquals("On my way", row.preview)
        assertFalse(row.outgoingPreview)
        assertEquals("12", row.unreadLabel)
        assertEquals("Morgan, 17:59, On my way, 12 unread", row.accessibilityLabel)
    }

    @Test
    fun outgoingPreviewAddsOnlyTheAvailableSentCheck() {
        val row = chat(
            lastMessage = ChatListPreview("Got it", now.epochSecond, true),
        ).toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)

        assertEquals("You: Got it", row.preview)
        assertTrue(row.outgoingPreview)
        assertEquals("Morgan, 18:00, Sent, You: Got it", row.accessibilityLabel)
    }

    @Test
    fun unavailableMessageFieldsStayAbsentAndDoNotInventReadState() {
        val noMessage = chat(lastMessage = null).toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)
        assertEquals("No messages yet", noMessage.preview)
        assertFalse(noMessage.outgoingPreview)
        assertNull(noMessage.timestamp)
        assertNull(noMessage.unreadLabel)

        val blankMessage = chat(
            lastMessage = ChatListPreview("  ", 0, true),
        ).toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)
        assertNull(blankMessage.preview)
        assertFalse(blankMessage.outgoingPreview)
        assertFalse(blankMessage.accessibilityLabel.contains("read", ignoreCase = true))
    }

    private fun chat(
        unreadCount: Long = 0,
        lastMessage: ChatListPreview?,
    ) = ChatListEntry(
        id = 45,
        title = "Morgan",
        unreadCount = unreadCount,
        lastMessage = lastMessage,
    )
}
