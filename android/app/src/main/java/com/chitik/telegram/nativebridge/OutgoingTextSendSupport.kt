package com.chitik.telegram.nativebridge

import java.security.SecureRandom
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONObject

// The positive counter starts at a random point so process restarts do not reuse MTProto random_id.
private val nextClientId = AtomicLong(SecureRandom().nextLong().ushr(1).coerceAtLeast(1L))

internal fun nextPositiveClientMessageId(): Long {
    while (true) {
        val previous = nextClientId.get()
        val next = if (previous == Long.MAX_VALUE) 1L else previous + 1L
        if (nextClientId.compareAndSet(previous, next)) return previous
    }
}

internal fun reliableTextCommand(clientId: Long, chatId: Long, text: String): String {
    require(clientId > 0L && chatId != 0L && text.isNotBlank())
    return JSONObject()
        .put("type", "send_text_reliable")
        .put("payload", JSONObject()
            .put("client_id", clientId)
            .put("chat_id", chatId)
            .put("text", text))
        .toString()
}

internal data class OutgoingTextSendUi(
    val clientId: Long,
    val chatId: Long,
    val text: String,
    val status: String,
    val reason: String? = null,
) {
    val isInFlight: Boolean
        get() = status == "pending" || status == "sending" ||
            status == "awaiting_confirmation" || status == "flood_wait"

    val isFailure: Boolean
        get() = status == "failed" || status == "rejected"
}

internal fun parseOutgoingTextSend(value: JSONObject?): OutgoingTextSendUi? {
    if (value == null) return null
    val clientId = value.optLong("client_id", 0L)
    val chatId = value.optLong("chat_id", 0L)
    if (clientId <= 0L || chatId == 0L || !value.has("text") || value.isNull("text")) return null
    val state = value.optJSONObject("state") ?: return null
    val status = state.optString("status")
    if (status !in setOf("pending", "sending", "awaiting_confirmation", "flood_wait", "sent", "failed", "rejected", "cancelled")) return null
    return OutgoingTextSendUi(clientId, chatId, value.optString("text"), status,
        state.optString("reason").takeIf(String::isNotBlank))
}

internal fun parseOutgoingTextOutbox(root: JSONObject?): Map<Long, OutgoingTextSendUi> {
    if (root == null) return emptyMap()
    return buildMap {
        val keys = root.keys()
        while (keys.hasNext()) {
            val key = keys.next()
            val send = parseOutgoingTextSend(root.optJSONObject(key)) ?: continue
            if (key.toLongOrNull() == send.clientId) put(send.clientId, send)
        }
    }
}

internal fun parseOutgoingTextSendEvent(payload: String): OutgoingTextSendUi? = runCatching {
    parseOutgoingTextSend(JSONObject(payload).optJSONObject("payload"))
}.getOrNull()

internal fun NativeUiState.applyOutgoingTextSend(send: OutgoingTextSendUi): NativeUiState {
    val active = activeTextSendId == send.clientId && activeTextSendChatId == send.chatId
    val updated = outbox + (send.clientId to send)
    if (!active) return copy(outbox = updated)
    return when {
        send.isFailure -> copy(
            outbox = updated,
            activeTextSendId = null,
            activeTextSendChatId = null,
            activeTextSendText = null,
            failedTextSendId = send.clientId,
            failedTextSendText = activeTextSendText,
            error = send.reason ?: "Отправка сообщения не удалась",
        )
        send.status == "sent" || send.status == "cancelled" -> copy(
            outbox = updated,
            activeTextSendId = null,
            activeTextSendChatId = null,
            activeTextSendText = null,
        )
        else -> copy(outbox = updated)
    }
}
