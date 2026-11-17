package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class MessageAccessibilityPresentationTest {
    @Test
    fun incomingMessageDescriptionUsesItsAvailableTextAndAttachmentFields() {
        val message = message(
            text = "  See you soon  ",
            timestamp = "14:35",
            forwardedFrom = "Morgan",
            attachments = listOf(attachment(MediaKindUi.Photo)),
        )

        assertEquals(
            "Входящее сообщение. Переслано от Morgan. See you soon. Фото. Время: 14:35",
            messageBubbleAccessibilityLabel(message),
        )
    }

    @Test
    fun outgoingReadDescriptionDoesNotInventReadStateWhenUnavailable() {
        val sent = message(isOutgoing = true, text = "Got it")
        assertEquals("Ваше сообщение. Got it", messageBubbleAccessibilityLabel(sent))
        assertFalse(messageBubbleAccessibilityLabel(sent).contains("Прочитано"))

        val read = sent.copy(isRead = true, timestamp = "09:00")
        assertEquals("Ваше сообщение. Got it. Время: 09:00. Прочитано", messageBubbleAccessibilityLabel(read))
    }

    @Test
    fun contextActionLabelMatchesExistingLongPressCallbackMode() {
        assertEquals("Открыть действия сообщения", messageContextActionLabel(selectionMode = false))
        assertEquals("Изменить выбор сообщения", messageContextActionLabel(selectionMode = true))
        assertNull(messageClickAccessibilityLabel(selectionMode = false))
        assertEquals("Выбрать или снять выбор сообщения", messageClickAccessibilityLabel(selectionMode = true))
    }

    private fun message(
        text: String = "",
        timestamp: String = "",
        isOutgoing: Boolean = false,
        isRead: Boolean = false,
        forwardedFrom: String? = null,
        attachments: List<MediaAttachmentUi> = emptyList(),
    ) = ConversationMessageUi(
        id = "17",
        chatId = "5",
        text = text,
        timestamp = timestamp,
        isOutgoing = isOutgoing,
        isRead = isRead,
        forwardedFrom = forwardedFrom,
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
