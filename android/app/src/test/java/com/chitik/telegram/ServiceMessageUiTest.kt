package com.chitik.telegram

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ServiceMessageUiTest {
    @Test
    fun mapsTypedActionsToLocalizedPresentation() {
        val cases = listOf(
            """{"type":"user_joined","user_id":12,"inviter_id":7}""" to "Пользователь 12 присоединился к группе",
            """{"type":"user_left","user_id":12}""" to "Пользователь 12 покинул группу",
            """{"type":"user_added","user_ids":[12,13]}""" to "Добавлены пользователи: 12, 13",
            """{"type":"title_changed","title":"Team"}""" to "Название группы изменено: Team",
            """{"type":"photo_changed","photo_id":8}""" to "Фото группы изменено",
            """{"type":"photo_removed"}""" to "Фото группы удалено",
            """{"type":"message_pinned"}""" to "Закреплено сообщение",
            """{"type":"group_created","title":"Team","user_ids":[12]}""" to "Создана группа «Team»",
            """{"type":"channel_created","title":"News"}""" to "Создан канал «News»",
            """{"type":"group_migrated_to_supergroup","supergroup_id":-100}""" to "Группа преобразована в супергруппу",
            """{"type":"channel_migrated_from_group","group_id":9,"title":"Team"}""" to "Канал создан из группы «Team»",
            """{"type":"history_cleared"}""" to "История чата очищена",
        )
        for ((action, text) in cases) {
            val message = JSONObject().put("service", JSONObject().put("action", JSONObject(action)))
            assertEquals(text, parseServiceAction(message)?.displayText())
        }
    }

    @Test
    fun malformedAndLegacyActionsNeverCrashMessageMapping() {
        assertEquals(ServiceActionUi.Unknown, parseServiceAction(JSONObject().put("service", "bad")))
        assertEquals(ServiceActionUi.Unknown, parseServiceAction(JSONObject().put("service_message", true)))
        assertEquals("Событие чата", parseServiceAction(JSONObject().put("service", JSONObject().put("action", JSONObject().put("type", "future_action"))))?.displayText())
        assertNull(parseServiceAction(JSONObject()))
    }
}
