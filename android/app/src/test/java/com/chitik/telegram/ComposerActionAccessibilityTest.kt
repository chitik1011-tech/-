package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class ComposerActionAccessibilityTest {
    @Test
    fun attachmentActionLabelMatchesAvailablePickers() {
        assertEquals(
            "Добавить фото, видео или файл",
            composerAttachmentActionAccessibilityLabel(
                hasPhotoVideoPicker = true,
                hasDocumentPicker = true,
            ),
        )
        assertEquals(
            "Добавить фото или видео",
            composerAttachmentActionAccessibilityLabel(
                hasPhotoVideoPicker = true,
                hasDocumentPicker = false,
            ),
        )
        assertEquals(
            "Добавить файл",
            composerAttachmentActionAccessibilityLabel(
                hasPhotoVideoPicker = false,
                hasDocumentPicker = true,
            ),
        )
    }

    @Test
    fun locationAndSendActionsHaveActionLabels() {
        assertEquals("Отправить геопозицию", composerLocationActionAccessibilityLabel())
        assertEquals("Отправить сообщение", composerSendActionAccessibilityLabel(sending = false))
        assertEquals("Отправка сообщения", composerSendActionAccessibilityLabel(sending = true))
    }
}
