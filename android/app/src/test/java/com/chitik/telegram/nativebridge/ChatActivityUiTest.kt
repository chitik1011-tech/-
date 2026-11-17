package com.chitik.telegram.nativebridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ChatActivityUiTest {
    @Test
    fun parsesTypingAndRecordingEventsAndRejectsMalformedPayloads() {
        val typing = parseChatActivityEvent(
            "chat_activity_changed",
            """{"type":"chat_activity_changed","payload":{"chat_id":-7,"user_id":9,"generation":2,"activity":"typing","ttl_ms":6000}}""",
        )
        assertEquals(ChatActivityKindUi.TYPING, typing?.kind)
        assertEquals(2L, typing?.generation)

        val recording = parseChatActivityEvent(
            "chat_activity_changed",
            """{"payload":{"chat_id":-7,"user_id":9,"generation":3,"activity":"recording_voice","ttl_ms":6000}}""",
        )
        assertEquals(ChatActivityKindUi.RECORDING_VOICE, recording?.kind)

        assertNull(parseChatActivityEvent("chat_activity_changed", "not-json"))
        assertNull(parseChatActivityEvent(
            "chat_activity_changed",
            """{"payload":{"chat_id":-7,"user_id":0,"generation":3,"activity":"typing"}}""",
        ))
        assertNull(parseChatActivityEvent(
            "chat_activity_changed",
            """{"payload":{"chat_id":-7,"user_id":9,"generation":3,"activity":"unknown","ttl_ms":6000}}""",
        ))
    }

    @Test
    fun staleEndAndExpiryDoNotClearNewerGeneration() {
        val changed = ChatActivityEventUi(-7, 9, 4, ChatActivityKindUi.TYPING)
        val state = NativeUiState().applyChatActivityEvent(changed)
        val staleEnd = state.applyChatActivityEvent(ChatActivityEventUi(-7, 9, 3, null))
        assertEquals(state.chatActivities, staleEnd.chatActivities)

        val expired = state.applyChatActivityEvent(ChatActivityEventUi(-7, 9, 4, null))
        assertEquals(emptyMap<Long, Map<Long, ChatActivityUi>>(), expired.chatActivities)
        val staleChanged = expired.applyChatActivityEvent(changed)
        assertEquals(emptyMap<Long, Map<Long, ChatActivityUi>>(), staleChanged.chatActivities)
    }

    @Test
    fun activityLineIsVisibleForRealActivityAndClearsWithSessionReplacement() {
        val state = NativeUiState().applyChatActivityEvent(
            ChatActivityEventUi(12, 8, 1, ChatActivityKindUi.RECORDING_VOICE),
        )
        assertEquals("записывает голосовое сообщение…", chatActivityStatus(state.chatActivities[12].orEmpty()))
        assertNull(chatActivityStatus(emptyMap()))
        assertEquals(emptyMap<Long, Map<Long, ChatActivityUi>>(), state.advanceSelectionGeneration().chatActivities)

        val active = state.copy(
            lastConnectionSessionGeneration = 2L,
            reactionConnectionToken = 2L to 3L,
        )
        val replaced = active.applyConnectionLifecycleAndGeneration(
            """{"payload":{"token":{"generation":3,"epoch":0},"event":{"type":"sync_started"}}}""",
        )
        assertEquals(emptyMap<Long, Map<Long, ChatActivityUi>>(), replaced.chatActivities)
        assertEquals(emptyMap<Long, Map<Long, ChatActivityGenerationUi>>(), replaced.chatActivityGenerations)
    }
}
