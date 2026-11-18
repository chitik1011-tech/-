package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MediaViewerSelectionTest {
    private fun attachment(
        index: Int,
        kind: MediaKindUi,
        filename: String? = null,
    ) = MediaAttachmentUi(
        index = index,
        kind = kind,
        fileReference = "$kind:$index",
        sizeBytes = 1,
        mimeType = when {
            kind != MediaKindUi.Document -> "image/jpeg"
            filename?.endsWith(".webp") == true -> "image/webp"
            else -> "application/pdf"
        },
        width = 10,
        height = 10,
        durationMs = null,
        filename = filename,
        downloadState = MediaDownloadUiState.NotDownloaded,
    )

    private fun message(
        chatId: String,
        id: String,
        vararg attachments: MediaAttachmentUi,
    ) = ConversationMessageUi(
        id = id,
        chatId = chatId,
        text = "media",
        timestamp = "",
        isOutgoing = false,
        attachments = attachments.toList(),
    )

    @Test
    fun viewerSelectionIncludesOnlyLoadedViewableMediaInCurrentChatOrder() {
        val firstPhoto = attachment(0, MediaKindUi.Photo)
        val secondPhoto = attachment(0, MediaKindUi.Photo)
        val video = attachment(1, MediaKindUi.Video)
        val pdf = attachment(2, MediaKindUi.Document, "report.pdf")
        val voice = attachment(0, MediaKindUi.Voice)
        val messages = listOf(
            message("-1", "6", firstPhoto, pdf),
            message("-2", "7", attachment(0, MediaKindUi.Photo)),
            message("-1", "7", secondPhoto, video, voice),
        )
        val key = mediaViewerKey(chatId = "-1", messageId = "7", attachmentIndex = 1)

        val selection = resolveMediaViewerSelection("-1", messages, key)

        assertEquals(MediaViewerItem(7L, video), selection?.current)
        assertEquals(MediaViewerItem(7L, secondPhoto), selection?.previous)
        assertNull(selection?.next)
        assertEquals(3, selection?.items?.size)
        assertEquals(MediaViewerItem(6L, firstPhoto), selection?.items?.first())
    }

    @Test
    fun imageDocumentsAreIncludedButOtherDocumentsAreNot() {
        val image = attachment(0, MediaKindUi.Document, "photo.webp")
        val pdf = attachment(1, MediaKindUi.Document, "report.pdf")
        val key = mediaViewerKey("1", "7", 0)

        val selection = resolveMediaViewerSelection("1", listOf(message("1", "7", image, pdf)), key)

        assertEquals(MediaViewerItem(7L, image), selection?.current)
        assertEquals(1, selection?.items?.size)
        assertNull(
            resolveMediaViewerSelection(
                "1",
                listOf(message("1", "7", image, pdf)),
                mediaViewerKey("1", "7", 1),
            ),
        )
    }

    @Test
    fun malformedMissingAndOtherChatKeysDoNotSelectMedia() {
        val photo = attachment(0, MediaKindUi.Photo)
        val message = message("-1", "7", photo)
        assertNull(resolveMediaViewerSelection("-1", listOf(message), null))
        assertNull(resolveMediaViewerSelection("-1", listOf(message), "-1:7"))
        assertNull(resolveMediaViewerSelection("-1", listOf(message), "-1:7:nope"))
        assertNull(
            resolveMediaViewerSelection(
                "-2",
                listOf(message("-2", "7", photo)),
                mediaViewerKey("-1", "7", 0),
            ),
        )
    }
}
