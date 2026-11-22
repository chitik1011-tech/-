package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class ReactionAccessibilityPresentationTest {
    @Test
    fun unselectedReactionAnnouncesCountAndAddAction() {
        assertEquals(
            "👍. Реакций: 12. Поставить реакцию.",
            reactionChipAccessibilityLabel(ConversationReactionUi("emoji", "👍", 12, false)),
        )
    }

    @Test
    fun selectedReactionAnnouncesHowToRemoveOwnVote() {
        assertEquals(
            "❤️. Реакций: 3. Снять свою реакцию.",
            reactionChipAccessibilityLabel(ConversationReactionUi("emoji", "❤️", 3, true)),
        )
    }

    @Test
    fun reactionPickerHasAUsefulAccessibleName() {
        assertEquals("Добавить реакцию", reactionPickerAccessibilityLabel)
    }
}
