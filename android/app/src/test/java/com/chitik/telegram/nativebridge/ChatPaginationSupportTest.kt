package com.chitik.telegram.nativebridge

import com.chitik.telegram.ChatListEntry
import com.chitik.telegram.ChatListPreview
import com.chitik.telegram.notifications.NotificationRouteResolution
import com.chitik.telegram.notifications.resolveNotificationRoute
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ChatPaginationSupportTest {
    @Test
    fun firstPageReplacesAndDeduplicatesWhileKeepingNewerExistingPreview() {
        val current = NativeUiState(
            chats = listOf(
                chat(1L, "old one", preview = ChatListPreview("newer live", 20L, false)),
                chat(2L, "removed by first page"),
            ),
            chatsLoaded = true,
            chatPagination = ChatPaginationUiState(loaded = true, exhausted = true),
        ).beginChatPageRequest(ChatPageRequest(cursor = null))

        val updated = current.reduceChatPageLoaded(
            page(
                requestedCursor = null,
                nextCursor = "3",
                chats = """[
                    {"id":1,"title":"updated one","unread_count":2,"last_message":{"text":"stale","timestamp_unix":10,"outgoing":true}},
                    {"id":3,"title":"three","unread_count":0},
                    {"id":3,"title":"three again","unread_count":1}
                ]""",
            ),
        )

        assertEquals(listOf(1L, 3L), updated.chats.map(ChatListEntry::id))
        assertEquals("updated one", updated.chats[0].title)
        assertEquals(ChatListPreview("newer live", 20L, false), updated.chats[0].lastMessage)
        assertEquals("three again", updated.chats[1].title)
        assertEquals("3", updated.chatPagination.nextCursor)
        assertFalse(updated.chatPagination.exhausted)
        assertEquals(ChatPageRequest("3"), updated.chatPagination.nextPageRequest())
        assertTrue(updated.chatsLoaded)
    }

    @Test
    fun unreadCountersAreParsedAndMissingPageValuesPreserveKnownCounters() {
        val current = NativeUiState(
            chats = listOf(
                chat(1L, "one").copy(unreadMentionsCount = 3, unreadReactionsCount = 4),
            ),
            chatsLoaded = true,
            chatPagination = ChatPaginationUiState(
                loaded = true,
                loading = true,
                requestedCursor = "1",
                nextCursor = "1",
            ),
        )

        val updated = current.reduceChatPageLoaded(
            page(
                requestedCursor = "1",
                nextCursor = null,
                chats = """[
                    {"id":1,"title":"one refreshed","unread_count":2,"unread_mentions_count":0},
                    {"id":2,"title":"two","unread_reactions_count":4294967296}
                ]""",
            ),
        )

        assertEquals(0L, updated.chats[0].unreadMentionsCount)
        assertEquals(4L, updated.chats[0].unreadReactionsCount)
        assertEquals(4_294_967_295L, updated.chats[1].unreadReactionsCount)
        assertNull(updated.chats[1].unreadMentionsCount)
    }

    @Test
    fun chatListRefreshPreservesMissingCountersButAcceptsReportedZero() {
        val existing = listOf(
            chat(1L, "one").copy(unreadMentionsCount = 3, unreadReactionsCount = 4),
        )
        val updated = mergeChatListRefresh(
            existing = existing,
            incomingChats = listOf(
                chat(1L, "one refreshed").copy(unreadMentionsCount = 0),
            ),
            currentCapabilities = emptyMap(),
        )

        assertEquals(0L, updated.single().unreadMentionsCount)
        assertEquals(4L, updated.single().unreadReactionsCount)
    }

    @Test
    fun laterPageAppendsAndDeduplicatesInServerOrderThenMarksExhaustion() {
        val current = NativeUiState(
            chats = listOf(chat(1L, "one"), chat(3L, "three", preview = ChatListPreview("kept", 7L, true))),
            chatsLoaded = true,
            chatPagination = ChatPaginationUiState(
                loaded = true,
                loading = true,
                requestedCursor = "3",
                nextCursor = "3",
            ),
        )

        val updated = current.reduceChatPageLoaded(
            page(
                requestedCursor = "3",
                nextCursor = null,
                chats = """[
                    {"id":3,"title":"three refreshed","unread_count":4},
                    {"id":4,"title":"four","unread_count":0},
                    {"id":4,"title":"four duplicate","unread_count":1}
                ]""",
            ),
        )

        assertEquals(listOf(1L, 3L, 4L), updated.chats.map(ChatListEntry::id))
        assertEquals("three refreshed", updated.chats[1].title)
        assertEquals(ChatListPreview("kept", 7L, true), updated.chats[1].lastMessage)
        assertEquals("four duplicate", updated.chats[2].title)
        assertTrue(updated.chatPagination.exhausted)
        assertNull(updated.chatPagination.nextCursor)
        assertNull(updated.chatPagination.nextPageRequest())
    }

    @Test
    fun pageFailurePreservesRetryCursorAndFirstPageFailureRetriesFromBeginning() {
        val laterPage = NativeUiState(
            chats = listOf(chat(1L, "one")),
            chatsLoaded = true,
            chatPagination = ChatPaginationUiState(
                loaded = true,
                loading = true,
                requestedCursor = "2",
                nextCursor = "2",
            ),
        ).reduceChatPageFailed(failedPage("2", "Temporary network issue"))

        assertEquals("Temporary network issue", laterPage.chatPagination.error)
        assertFalse(laterPage.chatPagination.loading)
        assertEquals(ChatPageRequest("2"), laterPage.chatPagination.retryPageRequest())
        assertFalse(laterPage.chatPagination.exhausted)
        assertEquals(listOf(1L), laterPage.chats.map(ChatListEntry::id))

        val firstPage = NativeUiState(
            chatsLoaded = true,
            chatPagination = ChatPaginationUiState(
                loaded = true,
                loading = true,
                requestedCursor = null,
                nextCursor = "old-cursor",
                exhausted = false,
            ),
        ).reduceChatPageFailed(failedPage(null, "First page failed"))
        assertEquals(ChatPageRequest(null), firstPage.chatPagination.retryPageRequest())
    }

    @Test
    fun malformedPageNeverConfirmsExhaustionOrDropsExistingChats() {
        val current = NativeUiState(
            chats = listOf(chat(9L, "nine")),
            chatsLoaded = true,
            chatPagination = ChatPaginationUiState(
                loaded = true,
                loading = true,
                requestedCursor = "9",
                nextCursor = "9",
            ),
        )

        val updated = current.reduceChatPageLoaded("not-json")

        assertEquals(listOf(9L), updated.chats.map(ChatListEntry::id))
        assertFalse(updated.chatPagination.exhausted)
        assertEquals("Invalid chat page response", updated.chatPagination.error)
        assertEquals(ChatPageRequest("9"), updated.chatPagination.retryPageRequest())
    }

    @Test
    fun repeatedNextCursorKeepsNotificationRoutePendingInsteadOfClaimingExhaustion() {
        val current = NativeUiState(
            chats = listOf(chat(5L, "five")),
            chatsLoaded = true,
            chatPagination = ChatPaginationUiState(
                loaded = true,
                loading = true,
                requestedCursor = "3",
                nextCursor = "3",
                consumedCursors = setOf("2"),
            ),
        )

        val updated = current.reduceChatPageLoaded(
            page(requestedCursor = "3", nextCursor = "2", chats = "[]"),
        )

        assertFalse(updated.chatPagination.exhausted)
        assertEquals("Telegram returned a repeated chat page cursor", updated.chatPagination.error)
        assertEquals(ChatPageRequest(null), updated.chatPagination.retryPageRequest())
        assertEquals(
            NotificationRouteResolution.Wait,
            resolveNotificationRoute(
                route = com.chitik.telegram.notifications.TelegramNotificationRoute(9L, 99L),
                authorized = true,
                accountIdentityResolved = true,
                activeAccountUserId = 99L,
                chatsLoaded = updated.chatsLoaded,
                availableChatIds = updated.chats.map(ChatListEntry::id),
                chatListExhausted = updated.chatPagination.exhausted,
            ),
        )
    }

    @Test
    fun restoredRustPaginationFailureRetriesFromFirstPageDespiteStaleNextCursor() {
        val restored = parseChatPaginationSnapshot(
            JSONObject(
                """{"loaded":true,"next_cursor":"old-cursor","consumed_cursors":["10","old-cursor"],"loading":false,"requested_cursor":null,"error":"first-page refresh failed"}""",
            ),
        )

        assertEquals(setOf("10", "old-cursor"), restored?.consumedCursors)
        assertEquals("old-cursor", restored?.nextCursor)
        assertFalse(restored?.exhausted ?: true)
        assertEquals(ChatPageRequest(null), restored?.retryPageRequest())
    }

    private fun chat(id: Long, title: String, preview: ChatListPreview? = null) =
        ChatListEntry(id, title, unreadCount = 0L, lastMessage = preview)

    private fun page(requestedCursor: String?, nextCursor: String?, chats: String): String {
        val requested = requestedCursor?.let { "\"$it\"" } ?: "null"
        val next = nextCursor?.let { "\"$it\"" } ?: "null"
        return """{"type":"chats_page_loaded","payload":{"requested_cursor":$requested,"next_cursor":$next,"chats":$chats}}"""
    }

    private fun failedPage(cursor: String?, error: String): String {
        val request = cursor?.let { "\"$it\"" } ?: "null"
        return """{"type":"chats_page_failed","payload":{"cursor":$request,"error":"$error"}}"""
    }
}
