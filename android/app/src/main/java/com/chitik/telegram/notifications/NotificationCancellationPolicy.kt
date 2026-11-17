package com.chitik.telegram.notifications

import org.json.JSONObject

/** A validated, chat-scoped native event that can retire a notification preview. */
internal sealed interface NotificationCancellation {
    val chatId: Long

    data class ReadThrough(override val chatId: Long, val maxMessageId: Long) : NotificationCancellation
    data class MessagesChanged(override val chatId: Long, val messageIds: Set<Long>) : NotificationCancellation
}

/** Parses only real inbox/read or exact-message mutation events for notification cleanup. */
internal object NotificationCancellationPolicy {
    fun fromEvent(type: String, payload: String): NotificationCancellation? = runCatching {
        val body = JSONObject(payload).optJSONObject("payload") ?: return@runCatching null
        when (type) {
            "chat_read", "read_receipt_updated" -> {
                if (type == "read_receipt_updated" && body.optString("direction") != "inbox") {
                    return@runCatching null
                }
                val chatId = body.longValue("chat_id") ?: return@runCatching null
                val maxMessageId = body.longValue("max_message_id") ?: return@runCatching null
                if (chatId == 0L || maxMessageId <= 0L) return@runCatching null
                NotificationCancellation.ReadThrough(chatId, maxMessageId)
            }
            "message_edited" -> {
                val chatId = body.longValue("chat_id") ?: return@runCatching null
                val messageId = body.longValue("id") ?: return@runCatching null
                if (chatId == 0L || messageId <= 0L) return@runCatching null
                NotificationCancellation.MessagesChanged(chatId, setOf(messageId))
            }
            "messages_deleted" -> {
                val chatId = body.longValue("chat_id") ?: return@runCatching null
                val ids = body.optJSONArray("message_ids") ?: return@runCatching null
                val messageIds = buildSet {
                    for (index in 0 until ids.length()) {
                        val id = ids.optLong(index)
                        if (id > 0L) add(id)
                    }
                }
                if (chatId == 0L || messageIds.isEmpty()) return@runCatching null
                NotificationCancellation.MessagesChanged(chatId, messageIds)
            }
            else -> null
        }
    }.getOrNull()

    /** A notification is read only when its message falls at or below the inbox boundary. */
    fun isReadByBoundary(notificationMessageId: Long?, maxMessageId: Long): Boolean =
        maxMessageId > 0L && (notificationMessageId == null || notificationMessageId <= maxMessageId)

    /** Missing IDs identify legacy previews; retire them only in the affected chat. */
    fun matchesChangedMessage(notificationMessageId: Long?, changedMessageIds: Set<Long>): Boolean =
        notificationMessageId == null || notificationMessageId in changedMessageIds

    private fun JSONObject.longValue(key: String): Long? = when (val value = opt(key)) {
        is Long -> value
        is Int -> value.toLong()
        else -> null
    }
}
