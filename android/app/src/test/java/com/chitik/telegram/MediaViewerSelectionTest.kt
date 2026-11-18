package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MediaViewerSelectionTest {
    private val attachment = MediaAttachmentUi(
        index = 0,
        kind = MediaKindUi.Photo,
        fileReference = "photo:1",
        sizeBytes = 1,
        mimeType = "image/jpeg",
        width = 10,
        height = 10,
        durationMs = null,
        filename = null,
        downloadState = MediaDownloadUiState.NotDownloaded,
    )

    private fun message(chatId: String) = ConversationMessageUi(
        id = "7",
        chatId = chatId,
        text = "photo",
        timestamp = "",
        isOutgoing = false,
        attachments = listOf(attachment),
    )

    @Test
    fun viewerSelectionResolvesOnlyInTheChatThatOpenedIt() {
        val key = mediaViewerKey(chatId = "-1", messageId = "7", attachmentIndex = 0)

        assertEquals(7L to attachment, resolveMediaViewerSelection("-1", listOf(message("-1")), key))
        assertNull(resolveMediaViewerSelection("-2", listOf(message("-2")), key))
    }

    @Test
    fun malformedOrMissingViewerKeysDoNotSelectMedia() {
        assertNull(resolveMediaViewerSelection("1", listOf(message("1")), null))
        assertNull(resolveMediaViewerSelection("1", listOf(message("1")), "1:7"))
        assertNull(resolveMediaViewerSelection("1", listOf(message("1")), "1:7:nope"))
    }
}
