package com.chitik.telegram

/** Drafts are scoped to account and chat because notification taps switch chats in-place. */
internal data class ChatDraftKey(val accountUserId: Long, val chatId: Long)

internal fun draftForChat(
    drafts: Map<ChatDraftKey, String>,
    accountUserId: Long,
    chatId: Long,
): String = drafts[ChatDraftKey(accountUserId, chatId)].orEmpty()

internal fun draftsForAccount(
    drafts: Map<ChatDraftKey, String>,
    accountUserId: Long,
): Map<ChatDraftKey, String> = drafts.filterKeys { it.accountUserId == accountUserId }

internal fun updateDraftForChat(
    drafts: Map<ChatDraftKey, String>,
    accountUserId: Long,
    chatId: Long,
    text: String,
): Map<ChatDraftKey, String> {
    if (chatId == 0L) return drafts
    val key = ChatDraftKey(accountUserId, chatId)
    return when {
        text.isEmpty() -> drafts - key
        else -> drafts + (key to text)
    }
}

internal fun saveChatDrafts(drafts: Map<ChatDraftKey, String>): Map<String, Any?> =
    drafts.mapKeys { (key, _) -> "${key.accountUserId}:${key.chatId}" }

internal fun restoreChatDrafts(saved: Map<String, Any?>): Map<ChatDraftKey, String> =
    saved.mapNotNull { (key, value) ->
        val parts = key.split(':', limit = 2)
        val accountUserId = parts.getOrNull(0)?.toLongOrNull() ?: return@mapNotNull null
        val chatId = parts.getOrNull(1)?.toLongOrNull() ?: return@mapNotNull null
        val text = value as? String ?: return@mapNotNull null
        ChatDraftKey(accountUserId, chatId) to text
    }.toMap()
