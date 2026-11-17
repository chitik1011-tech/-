package com.chitik.telegram

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Test

class ChatListDrawerActionTest {
    @Test
    fun chatsActionOnlyClosesDrawer() = runBlocking {
        val calls = mutableListOf<String>()

        performChatListDrawerAction(
            action = ChatListDrawerAction.CHATS,
            closeDrawer = { calls += "close" },
            onOpenContacts = { calls += "contacts" },
            onRequestLogout = { calls += "logout" },
        )

        assertEquals(listOf("close"), calls)
    }

    @Test
    fun contactsActionClosesBeforeOpeningContacts() = runBlocking {
        val calls = mutableListOf<String>()

        performChatListDrawerAction(
            action = ChatListDrawerAction.CONTACTS,
            closeDrawer = { calls += "close" },
            onOpenContacts = { calls += "contacts" },
            onRequestLogout = { calls += "logout" },
        )

        assertEquals(listOf("close", "contacts"), calls)
    }

    @Test
    fun logoutActionRequestsConfirmationOnlyAfterClosingDrawer() = runBlocking {
        val calls = mutableListOf<String>()

        performChatListDrawerAction(
            action = ChatListDrawerAction.LOG_OUT,
            closeDrawer = { calls += "close" },
            onOpenContacts = { calls += "contacts" },
            onRequestLogout = { calls += "logout" },
        )

        assertEquals(listOf("close", "logout"), calls)
    }
}
