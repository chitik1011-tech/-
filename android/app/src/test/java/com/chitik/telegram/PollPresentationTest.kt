package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PollPresentationTest {
    @Test
    fun singleChoiceSelectionReplacesPreviousSelection() {
        val first = listOf(1, 2)
        val second = listOf(3, 4)

        assertEquals(setOf(first), selectPollOption(emptySet(), first, multipleChoice = false))
        assertEquals(setOf(second), selectPollOption(setOf(first), second, multipleChoice = false))
    }

    @Test
    fun multipleChoiceSelectionCanAddAndRemoveOptions() {
        val first = listOf(1)
        val second = listOf(2)

        val both = selectPollOption(setOf(first), second, multipleChoice = true)
        assertEquals(setOf(first, second), both)
        assertEquals(setOf(second), selectPollOption(both, first, multipleChoice = true))
    }

    @Test
    fun calculatesVotePercentDeterministically() {
        assertEquals(67, pollPercent(voterCount = 2, totalVoters = 3))
        assertEquals(0, pollPercent(voterCount = 0, totalVoters = 3))
        assertEquals(100, pollPercent(voterCount = 3, totalVoters = 3))
    }

    @Test
    fun hidesPercentWhenTelegramDidNotProvideUsableTotals() {
        assertNull(pollPercent(voterCount = null, totalVoters = 3))
        assertNull(pollPercent(voterCount = 1, totalVoters = null))
        assertNull(pollPercent(voterCount = 1, totalVoters = 0))
    }
}
