package com.chitik.telegram.nativebridge

import org.json.JSONArray
import org.json.JSONObject

const val GLOBAL_MESSAGE_SEARCH_SCOPE = 0L
const val GLOBAL_MESSAGE_SEARCH_LIMIT = 50

data class GlobalMessageSearchResultUi(
    val messageId: Long,
    val chatId: Long,
    val senderId: Long,
    val text: String,
    val timestampUnix: Long,
)

data class GlobalMessageSearchUiState(
    val requestId: Long = 0L,
    val query: String = "",
    val loading: Boolean = false,
    val results: List<GlobalMessageSearchResultUi> = emptyList(),
    val error: String? = null,
)

internal fun globalMessageSearchCommand(requestId: Long, query: String): String =
    JSONObject()
        .put("type", "search_messages")
        .put(
            "payload",
            JSONObject()
                .put("request_id", requestId)
                .put("chat_id", GLOBAL_MESSAGE_SEARCH_SCOPE)
                .put("query", query.trim())
                .put("limit", GLOBAL_MESSAGE_SEARCH_LIMIT),
        )
        .toString()

internal fun parseGlobalMessageSearchResults(payload: String): Pair<Long, List<GlobalMessageSearchResultUi>>? =
    runCatching {
        val body = JSONObject(payload).optJSONObject("payload") ?: return@runCatching null
        val results = body.optJSONArray("messages") ?: JSONArray()
        val parsed = buildList {
            for (index in 0 until results.length()) {
                val message = results.optJSONObject(index) ?: continue
                val messageId = message.optLong("id")
                val chatId = message.optLong("chat_id")
                if (messageId <= 0L || chatId == 0L) continue
                val text = message.optString("text").ifBlank {
                    message.optString("media_caption").ifBlank { "Message attachment" }
                }
                add(
                    GlobalMessageSearchResultUi(
                        messageId = messageId,
                        chatId = chatId,
                        senderId = message.optLong("sender_id"),
                        text = text,
                        timestampUnix = message.optLong("timestamp_unix"),
                    ),
                )
            }
        }
        body.optLong("request_id") to parsed
    }.getOrNull()

internal fun parseGlobalMessageSearchFailure(payload: String): Pair<Long, String>? = runCatching {
    val body = JSONObject(payload).optJSONObject("payload") ?: return@runCatching null
    body.optLong("request_id") to body.optString("error").ifBlank { "Telegram message search failed" }
}.getOrNull()
