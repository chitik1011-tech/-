package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class ChatDraftStateTest {
    @Test
    fun switchingChatsKeepsEachDraftWithItsChat() {
        val chatOneDraft = updateDraftForChat(emptyMap(), accountUserId = 7L, chatId = -1L, text = "reply in one")
        val bothDrafts = updateDraftForChat(chatOneDraft, accountUserId = 7L, chatId = -2L, text = "reply in two")

        assertEquals("reply in one", draftForChat(bothDrafts, 7L, -1L))
        assertEquals("reply in two", draftForChat(bothDrafts, 7L, -2L))
        assertEquals("", draftForChat(bothDrafts, 7L, -3L))
        assertEquals("", draftForChat(bothDrafts, 8L, -1L))
    }

    @Test
    fun clearingOneDraftLeavesOtherChatsUntouched() {
        val drafts = mapOf(ChatDraftKey(7L, -1L) to "one", ChatDraftKey(7L, -2L) to "two")

        assertEquals(
            mapOf(ChatDraftKey(7L, -2L) to "two"),
            updateDraftForChat(drafts, 7L, -1L, ""),
        )
    }

    @Test
    fun resolvedAccountDropsOtherAccountsDrafts() {
        val drafts = mapOf(
            ChatDraftKey(7L, -1L) to "active account",
            ChatDraftKey(8L, -1L) to "previous account",
        )

        assertEquals(mapOf(ChatDraftKey(7L, -1L) to "active account"), draftsForAccount(drafts, 7L))
    }

    @Test
    fun savedActivityStateRoundTripsDraftsByChat() {
        val drafts = mapOf(
            ChatDraftKey(7L, -1L) to "first chat",
            ChatDraftKey(8L, -1L) to "same chat id, other account",
            ChatDraftKey(7L, 2L) to "second chat",
        )

        assertEquals(drafts, restoreChatDrafts(saveChatDrafts(drafts)))
    }
}
