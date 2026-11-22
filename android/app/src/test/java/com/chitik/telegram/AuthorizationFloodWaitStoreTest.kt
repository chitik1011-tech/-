package com.chitik.telegram

import com.chitik.telegram.nativebridge.authorizationFloodWaitDeadline
import com.chitik.telegram.nativebridge.authorizationFloodWaitDeadlineForBoot
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

    @Test
    fun sameBootKeepsMonotonicDeadlineIndependentOfWallClockChanges() {
        val deadlineElapsed = authorizationFloodWaitDeadline(nowMillis = 50_000L, seconds = 90)

        assertEquals(
            deadlineElapsed,
            authorizationFloodWaitDeadlineForBoot(
                storedDeadlineElapsedMillis = deadlineElapsed,
                storedBootCount = 12,
                currentBootCount = 12,
                durationSeconds = 90,
                nowElapsedMillis = 80_000L,
            ),
        )
        assertEquals(60, authorizationFloodWaitRemainingSeconds(deadlineElapsed, nowMillis = 80_000L))
    }

    @Test
    fun rebootRearmsOnlyTheOriginalFiniteWaitFromNewBoot() {
        val restoredDeadline = authorizationFloodWaitDeadlineForBoot(
            storedDeadlineElapsedMillis = 5_090_000L,
            storedBootCount = 12,
            currentBootCount = 13,
            durationSeconds = 90,
            nowElapsedMillis = 4_000L,
        )

        assertEquals(94_000L, restoredDeadline)
        assertEquals(90, authorizationFloodWaitRemainingSeconds(restoredDeadline, nowMillis = 4_000L))
        assertEquals(0, authorizationFloodWaitRemainingSeconds(restoredDeadline, nowMillis = 94_000L))
        assertEquals(
            restoredDeadline,
            authorizationFloodWaitDeadlineForBoot(
                storedDeadlineElapsedMillis = restoredDeadline,
                storedBootCount = 13,
                currentBootCount = 13,
                durationSeconds = 90,
                nowElapsedMillis = 20_000L,
            ),
        )
    }
}
