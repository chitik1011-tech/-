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
    fun reliableReplyUsesTheSameOutboxContractWithReplyTarget() {
        val wire = reliableTextCommand(124L, 99L, "reply", replyToMessageId = 77L)
        assertTrue(wire.contains("\"type\":\"send_text_reliable\""))
        assertTrue(wire.contains("\"client_id\":124"))
        assertTrue(wire.contains("\"reply_to_message_id\":77"))
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
    fun mediaOutboxParsesProgressRetryAndCancelState() {
        val uploading = parseOutgoingTextSendEvent(
            """{"type":"message_send_state_changed","payload":{"client_id":41,"chat_id":99,"text":"caption","media":{"kind":"photo","path":"/private/photo.jpg","file_name":"photo.jpg","mime_type":"image/jpeg","size_bytes":2048},"state":{"status":"uploading","bytes_transferred":1024,"total_bytes":2048}}}""",
        )
        assertNotNull(uploading)
        assertEquals(50, uploading?.progressPercent)
        assertTrue(uploading?.canCancel == true)
        assertFalse(uploading?.canRetry == true)

        val failed = parseOutgoingTextSendEvent(
            """{"type":"message_send_state_changed","payload":{"client_id":41,"chat_id":99,"text":"caption","media":{"kind":"photo","path":"/private/photo.jpg","file_name":"photo.jpg","mime_type":"image/jpeg","size_bytes":2048},"state":{"status":"failed","reason":"offline","retryable":true}}}""",
        )
        assertTrue(failed?.canRetry == true)
        assertFalse(failed?.canCancel == true)

        val awaitingConfirmation = parseOutgoingTextSendEvent(
            """{"type":"message_send_state_changed","payload":{"client_id":41,"chat_id":99,"text":"caption","media":{"kind":"photo","path":"/private/photo.jpg","file_name":"photo.jpg","mime_type":"image/jpeg","size_bytes":2048},"state":{"status":"awaiting_confirmation","reason":"server acknowledgement was lost"}}}""",
        )
        assertTrue(awaitingConfirmation?.canRetry == true)
    }

    @Test
    fun videoOutboxPreservesValidMetadataAndDropsInvalidDimensions() {
        val event = """{"type":"message_send_state_changed","payload":{"client_id":42,"chat_id":99,"text":"clip","media":{"kind":"video","path":"/private/clip.mp4","file_name":"clip.mp4","mime_type":"video/mp4","size_bytes":4096,"video_duration_ms":12345,"video_width":1920,"video_height":1080},"state":{"status":"failed","retryable":true}}}"""
        val send = parseOutgoingTextSendEvent(event)
        assertEquals(12_345L, send?.media?.videoDurationMs)
        assertEquals(1920, send?.media?.videoWidth)
        assertEquals(1080, send?.media?.videoHeight)

        val incomplete = event.replace("video_height", "ignored_video_height")
        val withoutDimensions = parseOutgoingTextSendEvent(incomplete)
        assertEquals(null, withoutDimensions?.media?.videoWidth)
        assertEquals(null, withoutDimensions?.media?.videoHeight)
        assertEquals(12_345L, withoutDimensions?.media?.videoDurationMs)
    }

    @Test
    fun mediaImportBlocksComposerUntilQueued() {
        assertTrue(NativeUiState(mediaImportInProgress = true).sendingText)
        assertFalse(NativeUiState(mediaImportInProgress = false).sendingText)
    }

    @Test
    fun unknownSendOutcomeReleasesComposerWithoutMakingTheSendRetryable() {
        val id = 31L
        val pending = NativeUiState(activeTextSendId = id, activeTextSendChatId = 99L, activeTextSendText = "hello")
        assertTrue(pending.sendingText)
        val sending = pending.applyOutgoingTextSend(send(id, "sending"))
        assertTrue(sending.sendingText)
        val waiting = sending.applyOutgoingTextSend(send(id, "awaiting_confirmation"))
        assertFalse(waiting.sendingText)
        assertEquals("awaiting_confirmation", waiting.outbox[id]?.status)
        assertFalse(waiting.outbox[id]?.canRetry == true)
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
    @Test
    fun bulkDeleteAcknowledgementResetsSendingFlag() {
        val pending = NativeUiState(deletingMessages = true, busy = true, deleteError = "old")
        val finished = pending.finishBulkDelete(99L, setOf(42L))
        assertFalse(finished.deletingMessages)
        assertFalse(finished.busy)
        assertNull(finished.deleteError)
    }

    @Test
    fun combinedLifecyclePreservesExistingReactionLogicAndResetsGeneration() {
        val initial = NativeUiState(
            selectionGeneration = 10L,
            lastConnectionSessionGeneration = 3L,
            deletingMessages = true,
            activeTextSendId = 31L,
            activeTextSendChatId = 99L,
            activeTextSendText = "hello",
        )
        val event = """{"type":"connection_lifecycle","payload":{"token":{"generation":4,"epoch":1},"event":{"type":"ready"}}}"""
        val updated = initial.applyConnectionLifecycleAndGeneration(event)
        assertEquals(11L, updated.selectionGeneration)
        assertFalse(updated.deletingMessages)
        assertFalse(updated.sendingText)
        assertEquals("hello", updated.failedTextSendText)
    }

    @Test
    fun malformedLifecycleAndOutboxDoNotCrashOrClearExistingMessages() {
        val initial = NativeUiState(selectionGeneration = 9L, lastConnectionSessionGeneration = 3L)
        assertEquals(initial, initial.applyConnectionLifecycleAndGeneration("{invalid"))
        assertNull(parseOutgoingTextSendEvent("[]"))
        assertTrue(parseOutgoingTextOutbox(null).isEmpty())
    }

}
