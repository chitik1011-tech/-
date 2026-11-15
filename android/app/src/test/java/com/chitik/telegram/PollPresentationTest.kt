package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PollPresentationTest {
    @Test
    fun calculatesVotePercentDeterministically() {
        assertEquals(67, pollPercent(voterCount = 2, totalVoters = 3))
        assertEquals(0, pollPercent(voterCount = 0, totalVoters = 3))
        assertEquals(100, pollPercent(voterCount = 3, totalVoters = 3))
    }

    @Test
    fun presentsTypedServiceActionsAsChatEvents() {
        assertEquals(
            "Название чата изменено: Team",
            serviceMessageLabel(
                ConversationServiceUi(
                    action = ConversationServiceAction.TitleChanged,
                    title = "Team",
                ),
            ),
        )
        assertEquals(
            "Закреплено сообщение 42",
            serviceMessageLabel(
                ConversationServiceUi(
                    action = ConversationServiceAction.MessagePinned,
                    targetId = 42,
                ),
            ),
        )
        assertEquals("Событие в чате", serviceMessageLabel(null))
    }

    @Test
    fun hidesPercentWhenTelegramDidNotProvideUsableTotals() {
        assertNull(pollPercent(voterCount = null, totalVoters = 3))
        assertNull(pollPercent(voterCount = 1, totalVoters = null))
        assertNull(pollPercent(voterCount = 1, totalVoters = 0))
    }
}
