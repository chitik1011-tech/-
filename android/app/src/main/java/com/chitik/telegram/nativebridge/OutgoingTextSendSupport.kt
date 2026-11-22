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

internal fun reliableTextCommand(
    clientId: Long,
    chatId: Long,
    text: String,
    replyToMessageId: Long? = null,
): String {
    require(clientId > 0L && chatId != 0L && text.isNotBlank())
    require(replyToMessageId == null || replyToMessageId > 0L)
    val payload = JSONObject()
        .put("client_id", clientId)
        .put("chat_id", chatId)
        .put("text", text)
    replyToMessageId?.let { payload.put("reply_to_message_id", it) }
    return JSONObject()
        .put("type", "send_text_reliable")
        .put("payload", payload)
        .toString()
}

/** Builds the established reliable media command, preserving the metadata required for a voice note. */
internal fun reliableMediaCommand(
    clientId: Long,
    chatId: Long,
    media: PickedOutgoingMedia,
    caption: String = "",
): String {
    require(clientId > 0L && chatId != 0L && media.sizeBytes > 0L)
    val attachment = JSONObject()
        .put("kind", media.kind)
        .put("path", media.path)
        .put("file_name", media.fileName)
        .put("mime_type", media.mimeType)
        .put("size_bytes", media.sizeBytes)
    media.voiceDurationMs?.let { attachment.put("voice_duration_ms", it) }
    media.voiceWaveform?.let { waveform ->
        attachment.put("voice_waveform", org.json.JSONArray(waveform))
    }
    media.videoDurationMs?.let { attachment.put("video_duration_ms", it) }
    media.videoWidth?.let { attachment.put("video_width", it) }
    media.videoHeight?.let { attachment.put("video_height", it) }
    return JSONObject()
        .put("type", "send_media_reliable")
        .put(
            "payload",
            JSONObject()
                .put("client_id", clientId)
                .put("chat_id", chatId)
                .put("media", attachment)
                .put("caption", caption),
        )
        .toString()
}

data class OutgoingMediaUi(
    val kind: String,
    val path: String,
    val fileName: String,
    val mimeType: String,
    val sizeBytes: Long,
    val videoDurationMs: Long? = null,
    val videoWidth: Int? = null,
    val videoHeight: Int? = null,
    val voiceDurationMs: Long? = null,
    val voiceWaveform: List<Int>? = null,
)

data class OutgoingTextSendUi(
    val clientId: Long,
    val chatId: Long,
    val text: String,
    val status: String,
    val reason: String? = null,
    val media: OutgoingMediaUi? = null,
    val bytesTransferred: Long = 0L,
    val totalBytes: Long? = null,
    val retryable: Boolean = false,
) {
    val isInFlight: Boolean
        get() = status == "pending" || status == "sending" || status == "uploading" ||
            status == "awaiting_confirmation" || status == "flood_wait"

    val isFailure: Boolean
        get() = status == "failed" || status == "rejected"

    val canRetry: Boolean
        get() = media != null && (
            status == "awaiting_confirmation" || (status == "failed" && retryable)
        )

    val canCancel: Boolean
        get() = status == "uploading" && media != null

    val progressPercent: Int?
        get() = if (status == "uploading" && totalBytes != null && totalBytes > 0L) {
            ((bytesTransferred.coerceIn(0L, totalBytes) * 100L) / totalBytes).toInt()
        } else null
}

internal fun parseOutgoingTextSend(value: JSONObject?): OutgoingTextSendUi? {
    if (value == null) return null
    val clientId = value.optLong("client_id", 0L)
    val chatId = value.optLong("chat_id", 0L)
    if (clientId <= 0L || chatId == 0L || !value.has("text") || value.isNull("text")) return null
    val state = value.optJSONObject("state") ?: return null
    val status = state.optString("status")
    if (status !in setOf("pending", "sending", "uploading", "awaiting_confirmation", "flood_wait", "sent", "failed", "rejected", "cancelled")) return null
    val mediaObject = value.optJSONObject("media")
    val media = mediaObject?.let { item ->
        val kind = item.optString("kind")
        val path = item.optString("path")
        val fileName = item.optString("file_name")
        val mimeType = item.optString("mime_type")
        val sizeBytes = item.optLong("size_bytes", 0L)
        val videoDurationMs = item.optLong("video_duration_ms", -1L)
            .takeIf { kind == "video" && it in 1..172_800_000L }
        val videoWidth = item.optInt("video_width", -1)
            .takeIf { kind == "video" && it in 1..32768 }
        val videoHeight = item.optInt("video_height", -1)
            .takeIf { kind == "video" && it in 1..32768 }
        val voiceDurationMs = item.optLong("voice_duration_ms", -1L)
            .takeIf { kind == "voice" && it in 1_000L..172_800_000L }
        val voiceWaveform = item.optJSONArray("voice_waveform")?.let { waveform ->
            if (kind != "voice" || waveform.length() !in 1..4096) return@let null
            (0 until waveform.length()).map { waveform.optInt(it, -1) }
                .takeIf { values -> values.all { it in 0..255 } }
        }
        val dimensions = if (videoWidth != null && videoHeight != null) {
            videoWidth to videoHeight
        } else {
            null
        }
        val voiceMetadataValid = kind != "voice" || (
            mimeType.equals("audio/ogg", ignoreCase = true) &&
                fileName.endsWith(".ogg", ignoreCase = true) && voiceDurationMs != null
            )
        if (kind in setOf("photo", "video", "voice", "document") && voiceMetadataValid && path.isNotBlank() &&
            fileName.isNotBlank() && mimeType.isNotBlank() && sizeBytes > 0L
        ) OutgoingMediaUi(
            kind,
            path,
            fileName,
            mimeType,
            sizeBytes,
            videoDurationMs,
            dimensions?.first,
            dimensions?.second,
            voiceDurationMs,
            voiceWaveform,
        ) else null
    }
    if (status == "uploading" && media == null) return null
    return OutgoingTextSendUi(
        clientId = clientId,
        chatId = chatId,
        text = value.optString("text"),
        status = status,
        reason = state.optString("reason").takeIf(String::isNotBlank),
        media = media,
        bytesTransferred = state.optLong("bytes_transferred", value.optLong("bytes_transferred", 0L)).coerceAtLeast(0L),
        totalBytes = state.optLong("total_bytes", 0L).takeIf { it > 0L }
            ?: media?.sizeBytes,
        retryable = state.optBoolean("retryable", false),
    )
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
            textSendError = send.reason ?: "Отправка сообщения не удалась",
        )
        send.status == "sent" || send.status == "cancelled" ||
            send.status == "awaiting_confirmation" -> copy(
                outbox = updated,
                activeTextSendId = null,
                activeTextSendChatId = null,
                activeTextSendText = null,
            )
        else -> copy(outbox = updated)
    }
}

internal fun NativeUiState.mediaSendForChat(chatId: Long): OutgoingTextSendUi? {
    val active = lastTextSendId?.let(outbox::get)
        ?.takeIf { it.chatId == chatId && it.media != null }
    return active ?: outbox.values
        .filter { it.chatId == chatId && it.media != null }
        .maxByOrNull { it.clientId }
}

internal fun NativeUiState.textSendStatusForChat(chatId: Long): String? {
    val send = lastTextSendId?.let(outbox::get)
        ?.takeIf { it.chatId == chatId }
        ?: mediaSendForChat(chatId)
        ?: return null
    if (send.chatId != chatId) return null
    return when (send.status) {
        "pending" -> "В очереди"
        "uploading" -> send.progressPercent?.let { "Загружается: $it%" } ?: "Загружается"
        "sending" -> if (send.media != null) "Отправляется в Telegram" else "Отправляется"
        "awaiting_confirmation" -> "Ожидается подтверждение Telegram"
        "flood_wait" -> "Ожидание ограничения Telegram"
        "sent" -> "Отправлено"
        "failed", "rejected" -> "Ошибка отправки"
        "cancelled" -> "Отменено"
        else -> null
    }
}
