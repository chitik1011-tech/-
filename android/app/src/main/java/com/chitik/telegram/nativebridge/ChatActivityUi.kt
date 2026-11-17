package com.chitik.telegram.nativebridge

import org.json.JSONObject

enum class ChatActivityKindUi {
    TYPING,
    RECORDING_VOICE,
    RECORDING_VIDEO,
    UPLOADING_MEDIA,
}

data class ChatActivityUi(
    val generation: Long,
    val kind: ChatActivityKindUi,
)

data class ChatActivityGenerationUi(
    val generation: Long,
    val active: Boolean,
)

internal data class ChatActivityEventUi(
    val chatId: Long,
    val userId: Long,
    val generation: Long,
    val kind: ChatActivityKindUi?,
)

internal fun parseChatActivityEvent(type: String, payload: String): ChatActivityEventUi? = runCatching {
    val body = JSONObject(payload).optJSONObject("payload") ?: return@runCatching null
    val chatId = body.optLong("chat_id", 0L)
    val userId = body.optLong("user_id", 0L)
    val generation = body.optLong("generation", 0L)
    if (chatId == 0L || userId <= 0L || generation <= 0L) return@runCatching null
    val kind = when (type) {
        "chat_activity_changed" -> {
            if (body.optLong("ttl_ms", 0L) <= 0L) return@runCatching null
            when (body.optString("activity")) {
                "typing" -> ChatActivityKindUi.TYPING
                "recording_voice" -> ChatActivityKindUi.RECORDING_VOICE
                "recording_video" -> ChatActivityKindUi.RECORDING_VIDEO
                "uploading_media" -> ChatActivityKindUi.UPLOADING_MEDIA
                else -> return@runCatching null
            }
        }
        "chat_activity_ended", "chat_activity_expired" -> null
        else -> return@runCatching null
    }
    ChatActivityEventUi(chatId, userId, generation, kind)
}.getOrNull()

internal fun NativeUiState.applyChatActivityEvent(event: ChatActivityEventUi): NativeUiState {
    val chat = chatActivities[event.chatId].orEmpty()
    val generations = chatActivityGenerations[event.chatId].orEmpty()
    val previous = generations[event.userId]
    if (previous != null && event.generation < previous.generation) return this

    val updatedActivities = if (event.kind == null) {
        val active = previous ?: return this
        if (active.generation != event.generation || !active.active) return this
        chat - event.userId
    } else {
        if (previous?.let { it.generation == event.generation && !it.active } == true) return this
        chat + (event.userId to ChatActivityUi(event.generation, event.kind))
    }
    val updatedGenerations = generations + (
        event.userId to ChatActivityGenerationUi(event.generation, active = event.kind != null)
    )
    val activities = if (updatedActivities.isEmpty()) chatActivities - event.chatId
    else chatActivities + (event.chatId to updatedActivities)
    return copy(
        chatActivities = activities,
        chatActivityGenerations = chatActivityGenerations + (event.chatId to updatedGenerations),
    )
}

internal fun chatActivityStatus(activities: Map<Long, ChatActivityUi>): String? {
    if (activities.isEmpty()) return null
    val kinds = activities.values.map(ChatActivityUi::kind)
    val distinctKinds = kinds.toSet()
    val label = when {
        distinctKinds.size > 1 -> "пользователи активны"
        distinctKinds.singleOrNull() == ChatActivityKindUi.TYPING -> "печатает"
        distinctKinds.singleOrNull() == ChatActivityKindUi.RECORDING_VOICE -> "записывает голосовое сообщение"
        distinctKinds.singleOrNull() == ChatActivityKindUi.RECORDING_VIDEO -> "записывает видео"
        else -> "отправляет медиа"
    }
    return if (activities.size > 1) "${activities.size} участника: $label…" else "$label…"
}
