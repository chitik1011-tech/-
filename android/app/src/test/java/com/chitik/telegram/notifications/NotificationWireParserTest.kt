package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationWireParserTest {
    @Test
    fun parsesLiveIncomingTextAndStableIdentity() {
        val parsed = NotificationWireParser.parseLiveMessage(
            """{"type":"message_added","payload":{"id":45,"chat_id":-300,"outgoing":false,"text":"Hello"}}""",
        )

        assertEquals(45L, parsed?.messageId)
        assertEquals(-300L, parsed?.chatId)
        assertEquals("Hello", parsed?.preview)
        assertFalse(parsed?.outgoing ?: true)
    }

    @Test
    fun parsesMediaAndServiceFallbacksAndPrefersCaptions() {
        val photo = NotificationWireParser.parseLiveMessage(
            """{"type":"message_added","payload":{"id":4,"chat_id":2,"outgoing":false,"attachments":[{"kind":"photo"}]}}""",
        )
        val captioned = NotificationWireParser.parseLiveMessage(
            """{"type":"message_added","payload":{"id":5,"chat_id":2,"outgoing":false,"text":"","media_caption":"Caption","attachments":[{"kind":"video"}]}}""",
        )
        val location = NotificationWireParser.parseLiveMessage(
            """{"type":"message_added","payload":{"id":6,"chat_id":2,"outgoing":false,"location":{"kind":"location"}}}""",
        )
        val service = NotificationWireParser.parseLiveMessage(
            """{"type":"message_added","payload":{"id":7,"chat_id":2,"outgoing":false,"service_message":true}}""",
        )

        assertEquals("Photo", photo?.preview)
        assertEquals("Caption", captioned?.preview)
        assertEquals("Location", location?.preview)
        assertEquals("Service message", service?.preview)
    }

    @Test
    fun rejectsMalformedUnrelatedOrIncompleteEvents() {
        val invalid = listOf(
            "",
            "not-json",
            "{}",
            """{"type":"message_edited","payload":{"id":1,"chat_id":2,"outgoing":false}}""",
            """{"payload":{"id":1,"chat_id":2,"outgoing":false}}""",
            """{"type":"message_added","payload":{}}""",
            """{"type":"message_added","payload":{"id":1,"chat_id":2,"outgoing":"false"}}""",
            """{"type":"message_added","payload":{"id":1,"chat_id":2,"outgoing":null}}""",
            """{"type":"message_added","payload":{"id":"1","chat_id":2,"outgoing":false}}""",
            """{"type":"message_added","payload":{"id":1.5,"chat_id":2,"outgoing":false}}""",
            """{"type":"message_added","payload":{"id":0,"chat_id":2,"outgoing":false}}""",
            """{"type":"message_added","payload":{"id":1,"chat_id":0,"outgoing":false}}""",
        )

        invalid.forEach { payload ->
            assertNull("Should reject $payload", NotificationWireParser.parseLiveMessage(payload))
        }
        assertNull(NotificationWireParser.parseLiveMessage("x".repeat(1_048_577)))
    }

    @Test
    fun parsesOutgoingForPlannerAndBoundsNotificationPreview() {
        val longText = "x".repeat(300)
        val parsed = NotificationWireParser.parseLiveMessage(
            """{"type":"message_added","payload":{"id":77,"chat_id":4,"outgoing":true,"text":"$longText"}}""",
        )

        assertTrue(parsed?.outgoing == true)
        assertEquals(256, parsed?.preview?.length)
        assertTrue(parsed?.preview?.endsWith("…") == true)
    }
}
