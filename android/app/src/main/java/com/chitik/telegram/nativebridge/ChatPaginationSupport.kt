package com.chitik.telegram.nativebridge

import com.chitik.telegram.ChatListEntry
import com.chitik.telegram.ChatListPreview
import org.json.JSONArray
import org.json.JSONObject

/** Request for one page of Rust-backed dialogs. A null cursor means the first page. */
data class ChatPageRequest(val cursor: String?, val limit: Int = 100)

/** Android's consumer-side view of Rust's opaque chat cursor state. */
data class ChatPaginationUiState(
    val loaded: Boolean = false,
    val loading: Boolean = false,
    val requestedCursor: String? = null,
    val nextCursor: String? = null,
    val consumedCursors: Set<String> = emptySet(),
    val exhausted: Boolean = false,
    val error: String? = null,
    val retryRequest: ChatPageRequest? = null,
)

private data class ChatPageUi(
    val requestedCursor: String?,
    val chats: List<ChatListEntry>,
    val nextCursor: String?,
)

internal fun ChatPaginationUiState.nextPageRequest(): ChatPageRequest? {
    val cursor = nextCursor ?: return null
    if (!loaded || loading || error != null || exhausted || cursor in consumedCursors) return null
    return ChatPageRequest(cursor)
}

internal fun ChatPaginationUiState.retryPageRequest(): ChatPageRequest? =
    retryRequest?.takeIf { !loading && error != null }

internal fun NativeUiState.beginChatPageRequest(request: ChatPageRequest): NativeUiState = copy(
    chatPagination = chatPagination.copy(
        loading = true,
        requestedCursor = request.cursor,
        error = null,
        retryRequest = null,
    ),
    busy = busy || !chatPagination.loaded,
    error = null,
)

internal fun NativeUiState.failChatPageRequest(
    request: ChatPageRequest,
    message: String,
): NativeUiState = copy(
    chatPagination = chatPagination.copy(
        loading = false,
        requestedCursor = null,
        nextCursor = request.cursor ?: chatPagination.nextCursor,
        exhausted = false,
        error = message,
        retryRequest = request,
    ),
    busy = false,
    error = if (chats.isEmpty()) message else null,
)

internal fun NativeUiState.reduceChatPageLoading(payload: String): NativeUiState {
    val cursor = parseNullableCursor(payload, "cursor")
        ?: return failChatPage(chatPagination.requestedCursor, "Invalid chat page loading response")
    return copy(
        chatPagination = chatPagination.copy(
            loading = true,
            requestedCursor = cursor.value,
            error = null,
            retryRequest = null,
        ),
        busy = busy || !chatPagination.loaded,
    )
}

internal fun NativeUiState.reduceChatPageLoaded(payload: String): NativeUiState {
    val page = parseChatPage(payload) ?:
        return failChatPage(chatPagination.requestedCursor, "Invalid chat page response")
    val currentPagination = chatPagination
    if (!currentPagination.loading || page.requestedCursor != currentPagination.requestedCursor) return this

    val firstPage = page.requestedCursor == null
    val previousCursors = if (firstPage) emptySet() else currentPagination.consumedCursors
    val consumed = previousCursors + listOfNotNull(page.requestedCursor)
    val repeatedNextCursor = page.nextCursor?.let { it in consumed } == true
    val chats = mergeChatPage(
        existing = chats,
        pageChats = page.chats,
        firstPage = firstPage,
        currentCapabilities = chatCapabilities,
    )
    val paginationError = if (repeatedNextCursor) "Telegram returned a repeated chat page cursor" else null
    return copy(
        chats = chats,
        chatsLoaded = true,
        chatPagination = currentPagination.copy(
            loaded = true,
            loading = false,
            requestedCursor = null,
            nextCursor = page.nextCursor?.takeUnless { repeatedNextCursor },
            consumedCursors = consumed,
            exhausted = page.nextCursor == null,
            error = paginationError,
            retryRequest = if (repeatedNextCursor) ChatPageRequest(null) else null,
        ),
        busy = false,
        error = if (paginationError != null && chats.isEmpty()) paginationError else null,
        lastEventType = "chats_page_loaded",
        lastEventPayload = "{\"count\":${page.chats.size},\"has_more\":${page.nextCursor != null}}",
    )
}

internal fun NativeUiState.reduceChatPageFailed(payload: String): NativeUiState {
    val cursor = parseNullableCursor(payload, "cursor")
    val body = runCatching { JSONObject(payload).optJSONObject("payload") }.getOrNull()
    if (cursor == null || body == null) {
        return failChatPage(chatPagination.requestedCursor, "Invalid chat page failure response")
    }
    if (!chatPagination.loading || cursor.value != chatPagination.requestedCursor) return this
    val failure = body.stringOrNull("error") ?: "Could not load chats"
    val repeatedCursor = cursor.value != null && cursor.value in chatPagination.consumedCursors
    val cursorJson = cursor.value?.let { JSONObject.quote(it) } ?: "null"
    val nextCursor = when {
        repeatedCursor -> null
        cursor.value != null -> cursor.value
        else -> chatPagination.nextCursor
    }
    return copy(
        chatPagination = chatPagination.copy(
            loading = false,
            requestedCursor = null,
            nextCursor = nextCursor,
            exhausted = false,
            error = failure,
            retryRequest = ChatPageRequest(if (repeatedCursor) null else cursor.value),
        ),
        busy = false,
        error = if (chats.isEmpty()) failure else null,
        lastEventType = "chats_page_failed",
        lastEventPayload = "{\"cursor\":$cursorJson}",
    )
}

private fun NativeUiState.failChatPage(cursor: String?, message: String): NativeUiState = copy(
    chatPagination = chatPagination.copy(
        loading = false,
        requestedCursor = null,
        exhausted = false,
        error = message,
        retryRequest = ChatPageRequest(cursor),
    ),
    busy = false,
    error = if (chats.isEmpty()) message else null,
    lastEventType = "chats_page_failed",
    lastEventPayload = "",
)

private data class NullableCursor(val value: String?)

private fun parseNullableCursor(payload: String, key: String): NullableCursor? = runCatching {
    val body = JSONObject(payload).optJSONObject("payload") ?: return null
    if (!body.has(key)) return null
    when (val value = body.opt(key)) {
        JSONObject.NULL -> NullableCursor(null)
        is String -> if (value.isNotBlank()) NullableCursor(value) else null
        else -> null
    }
}.getOrNull()

private fun parseChatPage(payload: String): ChatPageUi? = runCatching {
    val root = JSONObject(payload)
    if (root.optString("type") != "chats_page_loaded") return null
    val body = root.optJSONObject("payload") ?: return null
    val requested = body.requiredNullableCursor("requested_cursor") ?: return null
    val next = body.requiredNullableCursor("next_cursor") ?: return null
    val chatsJson = body.optJSONArray("chats") ?: return null
    ChatPageUi(
        requestedCursor = requested.value,
        chats = parseChatPageEntries(chatsJson),
        nextCursor = next.value,
    )
}.getOrNull()

private fun JSONObject.requiredNullableCursor(key: String): NullableCursor? {
    if (!has(key)) return null
    return when (val value = opt(key)) {
        JSONObject.NULL -> NullableCursor(null)
        is String -> if (value.isNotBlank()) NullableCursor(value) else null
        else -> null
    }
}

private fun parseChatPageEntries(values: JSONArray): List<ChatListEntry> = buildList {
    for (index in 0 until values.length()) {
        val chat = values.optJSONObject(index) ?: continue
        val id = chat.numberOrNull("id") ?: continue
        if (id == 0L) continue
        val lastMessage = chat.optJSONObject("last_message")?.let { message ->
            ChatListPreview(
                text = message.optString("text"),
                timestampUnix = message.numberOrNull("timestamp_unix") ?: 0L,
                outgoing = message.optBoolean("outgoing"),
            )
        }
        add(
            ChatListEntry(
                id = id,
                title = chat.optString("title").takeIf(String::isNotBlank) ?: "Chat",
                unreadCount = chat.numberOrNull("unread_count") ?: 0L,
                lastMessage = lastMessage,
            ),
        )
    }
}

private fun mergeChatPage(
    existing: List<ChatListEntry>,
    pageChats: List<ChatListEntry>,
    firstPage: Boolean,
    currentCapabilities: Map<Long, com.chitik.telegram.ChatCapabilitiesUi>,
): List<ChatListEntry> {
    val oldById = existing.associateBy(ChatListEntry::id)
    val merged = if (firstPage) mutableListOf() else existing.toMutableList()
    for (pageChat in pageChats) {
        val previous = oldById[pageChat.id]
        val incoming = pageChat.copy(
            lastMessage = previous?.lastMessage?.takeIf { old ->
                pageChat.lastMessage?.timestampUnix?.let { old.timestampUnix >= it } ?: true
            } ?: pageChat.lastMessage,
            capabilities = currentCapabilities[pageChat.id]
                ?: previous?.capabilities
                ?: pageChat.capabilities,
        )
        val existingIndex = merged.indexOfFirst { it.id == incoming.id }
        if (existingIndex >= 0) merged[existingIndex] = incoming
        else merged += incoming
    }
    return merged
}

internal fun parseChatPaginationSnapshot(snapshot: JSONObject?): ChatPaginationUiState? {
    snapshot ?: return null
    val loaded = snapshot.optBoolean("loaded")
    val loading = snapshot.optBoolean("loading")
    val requestedCursor = snapshot.stringOrNull("requested_cursor")
    val nextCursor = snapshot.stringOrNull("next_cursor")
    val error = snapshot.stringOrNull("error")
    val consumed = snapshot.optJSONArray("consumed_cursors")?.let { values ->
        buildSet {
            for (index in 0 until values.length()) {
                values.optString(index).takeIf(String::isNotBlank)?.let(::add)
            }
        }
    }.orEmpty()
    // Rust clears requested_cursor on completion and does not persist the failed cursor separately.
    // A restored error could therefore be from a first-page refresh while next_cursor is stale.
    val retry = if (!loading && error != null) ChatPageRequest(cursor = null) else null
    return ChatPaginationUiState(
        loaded = loaded,
        loading = loading,
        requestedCursor = requestedCursor,
        nextCursor = nextCursor,
        consumedCursors = consumed,
        exhausted = loaded && nextCursor == null && error == null,
        error = error,
        retryRequest = retry,
    )
}

private fun JSONObject.stringOrNull(key: String): String? = when (val value = opt(key)) {
    JSONObject.NULL -> null
    is String -> value.takeIf(String::isNotBlank)
    else -> null
}

private fun JSONObject.numberOrNull(key: String): Long? = when (val value = opt(key)) {
    is Number -> value.toLong()
    else -> null
}
