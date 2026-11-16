package com.chitik.telegram

import org.json.JSONObject

/** Locale-ready projection of a typed Telegram service action. */
sealed interface ServiceActionUi {
    data class UserJoined(val userId: Long?, val inviterId: Long?) : ServiceActionUi
    data class UserLeft(val userId: Long) : ServiceActionUi
    data class UserRemoved(val userId: Long) : ServiceActionUi
    data class UserAdded(val userIds: List<Long>) : ServiceActionUi
    data class TitleChanged(val title: String) : ServiceActionUi
    data class PhotoChanged(val photoId: Long?) : ServiceActionUi
    data object PhotoRemoved : ServiceActionUi
    data class MessagePinned(val messageId: Long?) : ServiceActionUi
    data class GroupCreated(val title: String, val userIds: List<Long>) : ServiceActionUi
    data class ChannelCreated(val title: String) : ServiceActionUi
    data class GroupMigrated(val supergroupId: Long) : ServiceActionUi
    data class ChannelMigrated(val groupId: Long, val title: String) : ServiceActionUi
    data object HistoryCleared : ServiceActionUi
    data object Unknown : ServiceActionUi
}

internal fun parseServiceAction(message: JSONObject): ServiceActionUi? {
    if (!message.has("service") || message.isNull("service")) {
        return if (message.optBoolean("service_message")) ServiceActionUi.Unknown else null
    }
    val service = message.optJSONObject("service")
        ?: return ServiceActionUi.Unknown
    val action = service.optJSONObject("action") ?: return ServiceActionUi.Unknown
    fun ids(key: String): List<Long> {
        val array = action.optJSONArray(key) ?: return emptyList()
        return buildList {
            for (index in 0 until array.length()) {
                val id = array.optLong(index, Long.MIN_VALUE)
                if (id != Long.MIN_VALUE && id != 0L) add(id)
            }
        }
    }
    return when (action.optString("type")) {
        "user_joined" -> ServiceActionUi.UserJoined(
            action.optLong("user_id").takeIf { action.has("user_id") && !action.isNull("user_id") },
            action.optLong("inviter_id").takeIf { action.has("inviter_id") && !action.isNull("inviter_id") },
        )
        "user_left" -> ServiceActionUi.UserLeft(action.optLong("user_id"))
        "user_removed" -> ServiceActionUi.UserRemoved(action.optLong("user_id"))
        "user_added" -> ServiceActionUi.UserAdded(ids("user_ids"))
        "title_changed" -> ServiceActionUi.TitleChanged(action.optString("title"))
        "photo_changed" -> ServiceActionUi.PhotoChanged(action.optLong("photo_id").takeIf { action.has("photo_id") && !action.isNull("photo_id") })
        "photo_removed" -> ServiceActionUi.PhotoRemoved
        "message_pinned" -> ServiceActionUi.MessagePinned(action.optLong("message_id").takeIf { action.has("message_id") && !action.isNull("message_id") })
        "group_created" -> ServiceActionUi.GroupCreated(action.optString("title"), ids("user_ids"))
        "channel_created" -> ServiceActionUi.ChannelCreated(action.optString("title"))
        "group_migrated_to_supergroup" -> ServiceActionUi.GroupMigrated(action.optLong("supergroup_id"))
        "channel_migrated_from_group" -> ServiceActionUi.ChannelMigrated(action.optLong("group_id"), action.optString("title"))
        "history_cleared" -> ServiceActionUi.HistoryCleared
        else -> ServiceActionUi.Unknown
    }
}

internal fun ServiceActionUi.displayText(): String = when (this) {
    is ServiceActionUi.UserJoined -> userId?.let { "Пользователь $it присоединился к группе" } ?: "Пользователь присоединился к группе"
    is ServiceActionUi.UserLeft -> "Пользователь $userId покинул группу"
    is ServiceActionUi.UserRemoved -> "Пользователь $userId удалён из группы"
    is ServiceActionUi.UserAdded -> "Добавлены пользователи: ${userIds.joinToString().ifBlank { "участники" }}"
    is ServiceActionUi.TitleChanged -> "Название группы изменено: $title"
    is ServiceActionUi.PhotoChanged -> "Фото группы изменено"
    ServiceActionUi.PhotoRemoved -> "Фото группы удалено"
    is ServiceActionUi.MessagePinned -> "Закреплено сообщение"
    is ServiceActionUi.GroupCreated -> "Создана группа «$title»"
    is ServiceActionUi.ChannelCreated -> "Создан канал «$title»"
    is ServiceActionUi.GroupMigrated -> "Группа преобразована в супергруппу"
    is ServiceActionUi.ChannelMigrated -> "Канал создан из группы «$title»"
    ServiceActionUi.HistoryCleared -> "История чата очищена"
    ServiceActionUi.Unknown -> "Событие чата"
}
