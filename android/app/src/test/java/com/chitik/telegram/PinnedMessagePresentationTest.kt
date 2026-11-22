package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class PinnedMessagePresentationTest {
    @Test
    fun usesPinnedMessageTextInsteadOfAnInternalMessageId() {
        val message = message("42", text = "  Meeting at 5  ")

        assertEquals("Закреплено: Meeting at 5", pinnedMessageBannerLabel("42", listOf(message), 1))
    }

    @Test
    fun describesMediaAndPluralPinnedCount() {
        val message = message(
            "42",
            attachments = listOf(attachment(MediaKindUi.Voice)),
        )

        assertEquals("Закреплено (3): Голосовое сообщение", pinnedMessageBannerLabel("42", listOf(message), 3))
    }

    @Test
    fun fallsBackToGenericLabelWhenPinnedMessageIsNotInLoadedHistory() {
        assertEquals(
            "Закреплено: Закреплённое сообщение",
            pinnedMessageBannerLabel("42", emptyList(), 0),
        )
    }

    private fun message(
        id: String,
        text: String = "",
        attachments: List<MediaAttachmentUi> = emptyList(),
    ) = ConversationMessageUi(
        id = id,
        chatId = "5",
        text = text,
        timestamp = "",
        isOutgoing = false,
        attachments = attachments,
    )

    private fun attachment(kind: MediaKindUi) = MediaAttachmentUi(
        index = 0,
        kind = kind,
        fileReference = "",
        sizeBytes = null,
        mimeType = null,
        width = null,
        height = null,
        durationMs = null,
        filename = null,
        downloadState = MediaDownloadUiState.NotDownloaded,
    )
}
