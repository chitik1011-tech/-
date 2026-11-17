package com.chitik.telegram

import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ChatListMessageSearchActionTest {
    @Test
    fun actionIsAbsentWhenNoMessageSearchHandlerIsProvided() {
        assertNull(chatListMessageSearchAction(null))
    }

    @Test
    fun actionInvokesTheProvidedMessageSearchHandler() {
        var opened = false
        val action = chatListMessageSearchAction { opened = true }

        assertNotNull(action)
        action?.onClick?.invoke()
        assertTrue(opened)
    }
}
