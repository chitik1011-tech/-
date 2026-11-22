package com.chitik.telegram

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ChatListComposeActionTest {
    @Test
    fun newChatActionIsAvailableOutsideChatSearch() {
        assertTrue(chatListShouldShowNewChatAction(searching = false))
    }

    @Test
    fun newChatActionStaysOutOfTheWayDuringChatSearch() {
        assertFalse(chatListShouldShowNewChatAction(searching = true))
    }
}
