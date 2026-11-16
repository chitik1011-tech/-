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
        return legacyServiceAction(message.optBoolean("service_message"))
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
    val type = action.optString("type").takeIf { action.has("type") && it.isNotBlank() }
    return mapServiceAction(
        type = type,
        userId = nullableLong(action, "user_id"),
        inviterId = nullableLong(action, "inviter_id"),
        userIds = ids("user_ids"),
        title = action.optString("title"),
        photoId = nullableLong(action, "photo_id"),
        messageId = nullableLong(action, "message_id"),
        supergroupId = action.optLong("supergroup_id"),
        groupId = action.optLong("group_id"),
    )
}

private fun nullableLong(action: JSONObject, key: String): Long? =
    action.optLong(key).takeIf { action.has(key) && !action.isNull(key) }

internal fun legacyServiceAction(isServiceMessage: Boolean): ServiceActionUi? =
    if (isServiceMessage) ServiceActionUi.Unknown else null

internal fun mapServiceAction(
    type: String?,
    userId: Long? = null,
    inviterId: Long? = null,
    userIds: List<Long> = emptyList(),
    title: String = "",
    photoId: Long? = null,
    messageId: Long? = null,
    supergroupId: Long = 0,
    groupId: Long = 0,
): ServiceActionUi = when (type) {
        "user_joined" -> ServiceActionUi.UserJoined(
            userId,
            inviterId,
        )
        "user_left" -> ServiceActionUi.UserLeft(userId ?: 0L)
        "user_removed" -> ServiceActionUi.UserRemoved(userId ?: 0L)
        "user_added" -> ServiceActionUi.UserAdded(userIds)
        "title_changed" -> ServiceActionUi.TitleChanged(title)
        "photo_changed" -> ServiceActionUi.PhotoChanged(photoId)
        "photo_removed" -> ServiceActionUi.PhotoRemoved
        "message_pinned" -> ServiceActionUi.MessagePinned(messageId)
        "group_created" -> ServiceActionUi.GroupCreated(title, userIds)
        "channel_created" -> ServiceActionUi.ChannelCreated(title)
        "group_migrated_to_supergroup" -> ServiceActionUi.GroupMigrated(supergroupId)
        "channel_migrated_from_group" -> ServiceActionUi.ChannelMigrated(groupId, title)
        "history_cleared" -> ServiceActionUi.HistoryCleared
        else -> ServiceActionUi.Unknown
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
