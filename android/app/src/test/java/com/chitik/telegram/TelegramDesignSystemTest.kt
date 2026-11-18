package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TelegramDesignSystemTest {
    @Test fun rowAndAvatarMaintainReadableDensity() {
        assertTrue(TelegramUiTokens.chatRowHeight.value >= TelegramUiTokens.avatar.value)
        assertTrue(TelegramUiTokens.dividerInset.value > TelegramUiTokens.avatar.value)
        assertTrue(TelegramUiTokens.iconTouchTarget.value >= 48f)
    }

    @Test fun chatListDividerMatchesTelegramDialogCellStart() {
        assertEquals(72f, TelegramUiTokens.chatListDividerInset.value)
        assertEquals(76f, TelegramUiTokens.dividerInset.value)
    }

    @Test fun messageBubbleHasDistinctTailCorner() {
        assertTrue(TelegramUiTokens.messageCorner.value > TelegramUiTokens.messageTailCorner.value)
        assertTrue(TelegramUiTokens.composerCorner.value > TelegramUiTokens.messageCorner.value)
    }
}
