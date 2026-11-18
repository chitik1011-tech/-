package com.chitik.telegram

import com.chitik.telegram.nativebridge.authorizationFloodWaitDeadline
import com.chitik.telegram.nativebridge.authorizationFloodWaitRemainingSeconds
import org.junit.Assert.assertEquals
import org.junit.Test

class AuthorizationFloodWaitStoreTest {
    @Test
    fun deadlineRestoresRemainingTimeAcrossViewModelRestart() {
        val deadline = authorizationFloodWaitDeadline(nowMillis = 10_000L, seconds = 47)

        assertEquals(47, authorizationFloodWaitRemainingSeconds(deadline, nowMillis = 10_000L))
        assertEquals(30, authorizationFloodWaitRemainingSeconds(deadline, nowMillis = 27_000L))
        assertEquals(1, authorizationFloodWaitRemainingSeconds(deadline, nowMillis = 56_001L))
        assertEquals(0, authorizationFloodWaitRemainingSeconds(deadline, nowMillis = 57_000L))
    }

    @Test
    fun roundsUpAndSaturatesWithoutOverflow() {
        assertEquals(2, authorizationFloodWaitRemainingSeconds(2_001L, nowMillis = 1_000L))
        assertEquals(Int.MAX_VALUE, authorizationFloodWaitRemainingSeconds(Long.MAX_VALUE, nowMillis = 0L))
        assertEquals(Long.MAX_VALUE, authorizationFloodWaitDeadline(Long.MAX_VALUE - 100L, seconds = 1))
    }
}