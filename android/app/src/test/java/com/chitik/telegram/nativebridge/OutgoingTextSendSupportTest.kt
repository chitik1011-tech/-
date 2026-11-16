package com.chitik.telegram.nativebridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class OutgoingTextSendSupportTest {
    private fun send(id: Long, status: String, reason: String? = null) =
        OutgoingTextSendUi(id, 99L, "hello", status, reason)

    @Test
    fun reliableTextWireUsesExistingCommandContract() {
        val wire = reliableTextCommand(123L, 99L, "hello")
        assertTrue(wire.contains("\"type\":\"send_text_reliable\""))
        assertTrue(wire.contains("\"client_id\":123"))
        assertTrue(wire.contains("\"chat_id\":99"))
        assertTrue(wire.contains("\"text\":\"hello\""))
    }

    @Test
    fun generatedClientIdsArePositiveAndDistinct() {
        val ids = (0 until 2000).map { nextPositiveClientMessageId() }
        assertTrue(ids.all { it > 0L })
        assertEquals(ids.size, ids.toSet().size)
    }

    @Test
    fun outboxAndEventsParseAndIgnoreMalformedData() {
        val outbox = """{"31":{"client_id":31,"chat_id":99,"text":"hello","state":{"status":"sending"}},"32":{"client_id":32,"chat_id":99,"text":"hello","state":{"status":"failed","reason":"offline","retryable":true}},"junk":{},"33":{"client_id":35,"chat_id":99,"text":"bad","state":{"status":"sent","message_id":4}}}"""
        val parsed = parseOutgoingTextOutbox(org.json.JSONObject(outbox))
        assertEquals(setOf(31L, 32L), parsed.keys)
        assertEquals("offline", parsed[32L]?.reason)
        assertEquals("sending", parseOutgoingTextSendEvent(
            """{"type":"message_send_state_changed","payload":{"client_id":31,"chat_id":99,"text":"hello","state":{"status":"sending"}}}"""
        )?.status)
        assertNull(parseOutgoingTextSendEvent("{broken"))
        assertNull(parseOutgoingTextSendEvent("""{"payload":{"client_id":0}}"""))
        assertTrue(parseOutgoingTextOutbox(org.json.JSONObject("""{"broken":[],"bad":1}""")).isEmpty())
    }

    @Test
    fun pendingSendingConfirmedReleasesComposer() {
        val id = 31L
        val pending = NativeUiState(activeTextSendId = id, activeTextSendChatId = 99L, activeTextSendText = "hello")
        assertTrue(pending.sendingText)
        val sending = pending.applyOutgoingTextSend(send(id, "sending"))
        assertTrue(sending.sendingText)
        val waiting = sending.applyOutgoingTextSend(send(id, "awaiting_confirmation"))
        assertTrue(waiting.sendingText)
        val confirmed = waiting.applyOutgoingTextSend(send(id, "sent"))
        assertFalse(confirmed.sendingText)
        assertNull(confirmed.failedTextSendText)
    }

    @Test
    fun pendingFailedRestoresTextAndOldFailuresDoNotBlockComposer() {
        val pending = NativeUiState(activeTextSendId = 31L, activeTextSendChatId = 99L, activeTextSendText = "hello")
        val unrelated = pending.applyOutgoingTextSend(send(30L, "failed", "old"))
        assertTrue(unrelated.sendingText)
        val failed = unrelated.applyOutgoingTextSend(send(31L, "failed", "offline"))
        assertFalse(failed.sendingText)
        assertEquals(31L, failed.failedTextSendId)
        assertEquals("hello", failed.failedTextSendText)
        assertNotNull(failed.textSendError)
    }

    @Test
    fun sessionGenerationResetsTransientActiveSendAndBulkDelete() {
        val state = NativeUiState(
            selectionGeneration = 2,
            lastConnectionSessionGeneration = 4,
            activeTextSendId = 31L,
            activeTextSendChatId = 99L,
            activeTextSendText = "hello",
            deletingMessages = true,
        )
        val updated = state.recordConnectionSessionGeneration(5)
        assertFalse(updated.sendingText)
        assertFalse(updated.deletingMessages)
        assertEquals(3L, updated.selectionGeneration)
    }
}
