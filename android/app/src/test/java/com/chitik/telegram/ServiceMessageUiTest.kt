package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ServiceMessageUiTest {
    @Test
    fun mapsTypedActionsToLocalizedPresentation() {
        val cases = listOf(
            mapServiceAction("user_joined", userId = 12, inviterId = 7) to "Пользователь 12 присоединился к группе",
            mapServiceAction("user_left", userId = 12) to "Пользователь 12 покинул группу",
            mapServiceAction("user_added", userIds = listOf(12, 13)) to "Добавлены пользователи: 12, 13",
            mapServiceAction("user_removed", userId = 13) to "Пользователь 13 удалён из группы",
            mapServiceAction("title_changed", title = "Team") to "Название группы изменено: Team",
            mapServiceAction("photo_changed", photoId = 8) to "Фото группы изменено",
            mapServiceAction("photo_removed") to "Фото группы удалено",
            mapServiceAction("message_pinned", messageId = 22) to "Закреплено сообщение",
            mapServiceAction("group_created", title = "Team", userIds = listOf(12)) to "Создана группа «Team»",
            mapServiceAction("channel_created", title = "News") to "Создан канал «News»",
            mapServiceAction("group_migrated_to_supergroup", supergroupId = -100) to "Группа преобразована в супергруппу",
            mapServiceAction("channel_migrated_from_group", groupId = 9, title = "Team") to "Канал создан из группы «Team»",
            mapServiceAction("history_cleared") to "История чата очищена",
        )
        for ((action, text) in cases) {
            assertEquals(text, action.displayText())
        }
    }

    @Test
    fun malformedAndLegacyActionsNeverCrashMessageMapping() {
        assertEquals(ServiceActionUi.Unknown, mapServiceAction(null))
        assertEquals(ServiceActionUi.Unknown, mapServiceAction("future_action"))
        assertEquals("Событие чата", mapServiceAction("future_action").displayText())
        assertEquals("Добавлены пользователи: участники", mapServiceAction("user_added").displayText())
        assertEquals(ServiceActionUi.Unknown, legacyServiceAction(true))
        assertNull(legacyServiceAction(false))
    }
}
