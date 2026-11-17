package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class NotificationEventGateTest {
    private val message =
        """{"type":"message_added","payload":{"id":41,"chat_id":-8,"outgoing":false,"text":"Hello"}}"""

    @Test
    fun rejectsStaleGenerationAndUnauthenticatedEventsThenAcceptsCurrentLiveMessageOnce() {
        val gate = NotificationEventGate()
        gate.reset(12L)

        assertNull(gate.incoming(11L, "message_added", message, authorized = true))
        assertNull(gate.incoming(12L, "message_added", message, authorized = false))
        assertEquals(
            NotificationWireParser.Incoming(-8L, 41L, false, "Hello"),
            gate.incoming(12L, "message_added", message, authorized = true),
        )
        assertNull(gate.incoming(12L, "message_added", message, authorized = true))
    }

    @Test
    fun ignoresOtherEventKindsAndOutgoingMessages() {
        val gate = NotificationEventGate()
        gate.reset(12L)
        assertNull(gate.incoming(12L, "message_edited", message, authorized = true))
        assertNull(
            gate.incoming(
                12L,
                "message_added",
                """{"type":"message_added","payload":{"id":42,"chat_id":-8,"outgoing":true,"text":"mine"}}""",
                authorized = true,
            ),
        )
    }
}
