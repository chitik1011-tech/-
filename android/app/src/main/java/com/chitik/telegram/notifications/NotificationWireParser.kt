package com.chitik.telegram.notifications

import org.json.JSONObject

/** Strictly parses a live JNI message-added envelope; snapshot/history is never passed here. */
internal object NotificationWireParser {
    data class Incoming(
        val chatId: Long,
        val messageId: Long,
        val outgoing: Boolean,
        val preview: String,
    )

    fun parseLiveMessage(payload: String): Incoming? = runCatching {
        val root = JSONObject(payload)
        val message = root.optJSONObject("payload") ?: return@runCatching null
        if (!message.has("outgoing") || message.isNull("outgoing")) return@runCatching null
        val outgoing = message.opt("outgoing") as? Boolean ?: return@runCatching null
        val chatId = message.optLong("chat_id", 0L)
        val id = message.optLong("id", 0L)
        if (chatId == 0L || id <= 0L) return@runCatching null
        val kind = message.optJSONArray("attachments")?.optJSONObject(0)?.optString("kind")
        val fallback = when (kind) {
            "photo" -> "Photo"
            "video" -> "Video"
            "voice" -> "Voice message"
            "document" -> "Document"
            "sticker" -> "Sticker"
            "animation" -> "Animation"
            else -> when {
                message.has("location") && !message.isNull("location") -> "Location"
                message.optBoolean("service_message", false) -> "Service message"
                message.optJSONObject("reply_to") != null -> "Reply"
                message.optJSONObject("forwarded_from") != null -> "Forwarded message"
                else -> "New message"
            }
        }
        val preview = message.optString("text").ifBlank {
            message.optString("media_caption").ifBlank { fallback }
        }
        Incoming(chatId, id, outgoing, preview)
    }.getOrNull()
}
