package com.chitik.telegram.notifications

import org.json.JSONObject

/** Parses a bounded live `message_added` payload into notification-safe message fields. */
internal object NotificationWireParser {
    private const val MAX_PAYLOAD_LENGTH = 1_048_576
    private const val MAX_PREVIEW_LENGTH = 256

    data class Incoming(
        val chatId: Long,
        val messageId: Long,
        val outgoing: Boolean,
        val preview: String,
    )

    /** Returns null for malformed, unrelated, incomplete, or oversized event payloads. */
    fun parseLiveMessage(payload: String): Incoming? {
        if (payload.length > MAX_PAYLOAD_LENGTH) return null
        return runCatching {
            val root = JSONObject(payload)
            if (root.opt("type") != "message_added") return@runCatching null
            val message = root.optJSONObject("payload") ?: return@runCatching null
            val outgoing = message.opt("outgoing") as? Boolean ?: return@runCatching null
            val chatId = message.longOrNull("chat_id") ?: return@runCatching null
            val messageId = message.longOrNull("id") ?: return@runCatching null
            if (chatId == 0L || messageId <= 0L) return@runCatching null

            val attachmentKind = message.optJSONArray("attachments")
                ?.optJSONObject(0)
                ?.opt("kind") as? String
            val fallback = when (attachmentKind) {
                "photo" -> "Photo"
                "video" -> "Video"
                "voice" -> "Voice message"
                "document" -> "Document"
                "sticker" -> "Sticker"
                "animation" -> "Animation"
                else -> when {
                    message.optJSONObject("location") != null -> "Location"
                    message.opt("service_message") == true -> "Service message"
                    message.optJSONObject("reply_to") != null -> "Reply"
                    message.optJSONObject("forwarded_from") != null -> "Forwarded message"
                    else -> "New message"
                }
            }
            val preview = (message.opt("text") as? String)
                ?.takeIf(String::isNotBlank)
                ?: (message.opt("media_caption") as? String)
                    ?.takeIf(String::isNotBlank)
                ?: fallback

            Incoming(
                chatId = chatId,
                messageId = messageId,
                outgoing = outgoing,
                preview = preview.boundedPreview(),
            )
        }.getOrNull()
    }

    private fun JSONObject.longOrNull(key: String): Long? = when (val value = opt(key)) {
        is Long -> value
        is Int -> value.toLong()
        else -> null
    }

    private fun String.boundedPreview(): String =
        if (length <= MAX_PREVIEW_LENGTH) this else take(MAX_PREVIEW_LENGTH - 1) + "…"
}
