package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class ChatListSearchTest {
    private val chats = listOf(
        ChatListEntry(1L, "Alice", 1L, ChatListPreview("Lunch tomorrow", 100L, false)),
        ChatListEntry(2L, "Team", 0L, ChatListPreview("Release notes", 101L, true)),
    )

    @Test fun emptyQueryPreservesOriginalOrder() {
        assertEquals(listOf(1L, 2L), filterLoadedChats(chats, " ").map { it.id })
    }

    @Test fun titleAndMessageAreBothSearchable() {
        assertEquals(listOf(1L), filterLoadedChats(chats, "aliCE").map { it.id })
        assertEquals(listOf(2L), filterLoadedChats(chats, "RELEASE").map { it.id })
    }

    @Test fun noMatchesDoesNotFabricateChats() {
        assertEquals(emptyList<ChatListEntry>(), filterLoadedChats(chats, "missing"))
    }
}
