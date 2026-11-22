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
    fun chatRowSemanticsExposeSelectionStateWithoutChangingItsDescription() {
        val presentation = chat(lastMessage = null).toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)

        val selected = chatListRowSemantics(presentation, selected = true)
        assertEquals(presentation.accessibilityLabel, selected.accessibilityLabel)
        assertTrue(selected.selected)

        val unselected = chatListRowSemantics(presentation, selected = false)
        assertEquals(presentation.accessibilityLabel, unselected.accessibilityLabel)
        assertFalse(unselected.selected)
    }

    @Test
    fun mentionsAndReactionsHaveBoundedLabelsAndAccessibleDescriptions() {
        val row = chat(
            unreadMentionsCount = 1_000,
            unreadReactionsCount = 2,
            lastMessage = ChatListPreview("Ping", now.epochSecond, false),
        ).toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)

        assertEquals("999+", row.unreadMentionsLabel)
        assertEquals("2", row.unreadReactionsLabel)
        assertTrue(row.accessibilityLabel.contains("999+ unread mentions"))
        assertTrue(row.accessibilityLabel.contains("2 unread reactions"))
    }

    @Test
    fun absentZeroAndNegativeCounterValuesDoNotShowBadges() {
        val defaults = chat(lastMessage = null).toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)
        assertNull(defaults.unreadMentionsLabel)
        assertNull(defaults.unreadReactionsLabel)

        val noUnread = chat(
            unreadMentionsCount = 0,
            unreadReactionsCount = -1,
            lastMessage = null,
        ).toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)
        assertNull(noUnread.unreadMentionsLabel)
        assertNull(noUnread.unreadReactionsLabel)
    }

    @Test
    fun manualUnreadMarkHasSeparateAccessibleIndicator() {
        val marked = chat(isUnread = true, lastMessage = null)
            .toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)
        assertEquals("Marked unread", marked.markedUnreadLabel)
        assertTrue(marked.accessibilityLabel.contains("Marked unread"))

        val unmarked = chat(lastMessage = null)
            .toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)
        assertNull(unmarked.markedUnreadLabel)
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
    fun activeTelegramChatActivityReplacesMessagePreviewUntilItEnds() {
        val row = chat(
            lastMessage = ChatListPreview("Last message", now.epochSecond, true),
            activityStatus = "печатает…",
        ).toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)

        assertEquals("печатает…", row.preview)
        assertTrue(row.isActivityStatus)
        assertFalse(row.outgoingPreview)
        assertEquals("Morgan, 18:00, печатает…", row.accessibilityLabel)

        val idle = chat(
            lastMessage = ChatListPreview("Last message", now.epochSecond, true),
            activityStatus = " ",
        ).toChatListRowPresentation(now, ZoneOffset.UTC, Locale.US)
        assertEquals("You: Last message", idle.preview)
        assertFalse(idle.isActivityStatus)
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
        unreadMentionsCount: Long? = null,
        unreadReactionsCount: Long? = null,
        isUnread: Boolean? = null,
        activityStatus: String? = null,
        lastMessage: ChatListPreview?,
    ) = ChatListEntry(
        id = 45,
        title = "Morgan",
        unreadCount = unreadCount,
        lastMessage = lastMessage,
        unreadMentionsCount = unreadMentionsCount,
        unreadReactionsCount = unreadReactionsCount,
        isUnread = isUnread,
        activityStatus = activityStatus,
    )
}
