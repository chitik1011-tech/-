package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class MediaAttachmentAccessibilityTest {
    @Test
    fun photoOpenActionIncludesTheSanitizedFilename() {
        assertEquals(
            "Открыть фото summer.jpg",
            mediaAttachmentOpenAccessibilityLabel(attachment(MediaKindUi.Photo, "/private/cache/summer.jpg")),
        )
    }

    @Test
    fun photoOpenActionHasClearFallbackWhenFilenameIsMissing() {
        assertEquals(
            "Открыть фото",
            mediaAttachmentOpenAccessibilityLabel(attachment(MediaKindUi.Photo, null)),
        )
    }

    @Test
    fun inlineImageDocumentIsDescribedAsAnImage() {
        assertEquals(
            "Открыть изображение scan.png",
            mediaAttachmentOpenAccessibilityLabel(attachment(MediaKindUi.Document, "../scan.png")),
        )
    }

    private fun attachment(kind: MediaKindUi, filename: String?) = MediaAttachmentUi(
        index = 0,
        kind = kind,
        fileReference = "",
        sizeBytes = null,
        mimeType = null,
        width = null,
        height = null,
        durationMs = null,
        filename = filename,
        downloadState = MediaDownloadUiState.NotDownloaded,
    )
}
