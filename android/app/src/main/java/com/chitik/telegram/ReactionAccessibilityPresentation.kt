package com.chitik.telegram

/** Screen-reader action and count for one tappable Telegram reaction. */
internal fun reactionChipAccessibilityLabel(reaction: ConversationReactionUi): String = buildString {
    append(reaction.display)
    append(". Реакций: ${reaction.count}.")
    append(
        if (reaction.chosenByMe) " Снять свою реакцию."
        else " Поставить реакцию.",
    )
}

internal const val reactionPickerAccessibilityLabel = "Добавить реакцию"
