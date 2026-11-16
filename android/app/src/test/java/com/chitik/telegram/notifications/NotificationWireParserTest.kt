package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationWireParserTest {
    @Test fun parsesIncomingTextAndMessageIdentity() {
        val message = NotificationWireParser.parseLiveMessage(
            """{"type":"message_added","payload":{"id":45,"chat_id":-300,"outgoing":false,"text":"Hello"}}"""
        )
        assertEquals(-300L, message?.chatId)
        assertEquals(45L, message?.messageId)
        assertEquals("Hello", message?.preview)
        assertFalse(message?.outgoing ?: true)
    }

    @Test fun parsesVoiceAndOtherMediaFallback() {
        val voice = NotificationWireParser.parseLiveMessage(
            """{"payload":{"id":4,"chat_id":2,"outgoing":false,"text":"","attachments":[{"kind":"voice"}]}}"""
        )
        assertEquals("Voice message", voice?.preview)
        val location = NotificationWireParser.parseLiveMessage(
            """{"payload":{"id":5,"chat_id":2,"outgoing":false,"location":{"kind":"location"}}}"""
        )
        assertEquals("Location", location?.preview)
    }

    @Test fun rejectsMalformedIncompleteOrUntrustedOutgoing() {
        for (payload in listOf(
            "", "not-json", "{}", """{"payload":{}}""",
            """{"payload":{"id":1,"chat_id":2,"outgoing":"false"}}""",
            """{"payload":{"id":0,"chat_id":2,"outgoing":false}}""",
            """{"payload":{"id":1,"chat_id":0,"outgoing":false}}""",
            """{"payload":{"id":1,"chat_id":2,"outgoing":null}}""",
        )) assertNull(NotificationWireParser.parseLiveMessage(payload))
    }

    @Test fun outgoingIsParsedButPlannerMustSuppressIt() {
        val message = NotificationWireParser.parseLiveMessage(
            """{"payload":{"id":77,"chat_id":4,"outgoing":true,"text":"sent"}}"""
        )
        assertTrue(message?.outgoing == true)
        val planner = NotificationDecisionPlanner()
        planner.reset(1)
        assertFalse(
            planner.shouldNotify(
                NotificationDecisionPlanner.Identity(1, 4, 77),
                outgoing = message!!.outgoing, liveUpdate = true
            )
        )
    }
}
