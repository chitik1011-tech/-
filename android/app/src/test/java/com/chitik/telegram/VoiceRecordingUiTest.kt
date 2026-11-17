package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class VoiceRecordingUiTest {
    @Test
    fun elapsedTimeUsesCompactMinutesAndHours() {
        assertEquals("0:00", formatVoiceRecordingDuration(0L))
        assertEquals("0:59", formatVoiceRecordingDuration(59_999L))
        assertEquals("1:00", formatVoiceRecordingDuration(60_000L))
        assertEquals("1:00:00", formatVoiceRecordingDuration(3_600_000L))
    }

    @Test
    fun invalidNegativeElapsedTimeStartsAtZero() {
        assertEquals("0:00", formatVoiceRecordingDuration(-1L))
    }

    @Test
    fun permissionResultStartsOnlyTheChatThatIsStillSelected() {
        assertEquals(42L, voiceRecordingChatAfterPermission(true, 42L, 42L))
        assertEquals(null, voiceRecordingChatAfterPermission(false, 42L, 42L))
        assertEquals(null, voiceRecordingChatAfterPermission(true, 42L, -7L))
        assertEquals(null, voiceRecordingChatAfterPermission(true, 0L, 0L))
    }

    @Test
    fun pendingPermissionRequestIsDiscardedOnNavigationAway() {
        assertEquals(0L, pendingVoicePermissionChatAfterNavigation(42L, -7L))
        assertEquals(42L, pendingVoicePermissionChatAfterNavigation(42L, 42L))
        assertEquals(0L, pendingVoicePermissionChatAfterNavigation(0L, 42L))
    }

    @Test
    fun chatSwitchCancelsOnlyAnActiveRecordingForThePreviousChat() {
        assertEquals(true, shouldCancelVoiceRecordingForChatChange(42L, -7L))
        assertEquals(false, shouldCancelVoiceRecordingForChatChange(42L, 42L))
        assertEquals(false, shouldCancelVoiceRecordingForChatChange(null, 42L))
    }
}
