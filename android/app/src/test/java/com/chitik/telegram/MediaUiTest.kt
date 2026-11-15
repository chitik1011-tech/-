package com.chitik.telegram

import java.io.File
import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaUiTest {

    @Test
    fun filenameSanitizationNeverProducesTraversalPath() {
        assertEquals("photo.jpg", sanitizeDisplayFilename("../../private/photo.jpg"))
        assertEquals("report.pdf", sanitizeDisplayFilename("..\\..\\report.pdf"))
        assertEquals("file", sanitizeDisplayFilename("../.."))
        assertFalse(sanitizeDisplayFilename("a/b.txt").contains('/'))
        assertFalse(sanitizeDisplayFilename("a\\b.txt").contains('\\'))
    }

    @Test
    fun mimeSelectionPrefersValidDeclaredTypeAndFallsBackSafely() {
        assertEquals("application/pdf", resolveSafeMimeType("application/pdf", "ignored.bin"))
        assertEquals("video/mp4", resolveSafeMimeType("not a mime", "clip.mp4"))
        assertEquals("application/octet-stream", resolveSafeMimeType(null, "../../unknown.data"))
    }

    @Test
    fun openIntentSpecUsesNarrowAppAuthorityAndReadGrant() {
        val spec = buildOpenIntentSpec(
            packageName = "com.chitik.telegram",
            declaredMime = "application/pdf",
            filename = "paper.pdf",
        )

        assertEquals("android.intent.action.VIEW", spec.action)
        assertEquals("com.chitik.telegram.media", spec.authority)
        assertEquals("application/pdf", spec.mimeType)
        assertTrue(spec.grantReadPermission)
    }

    @Test
    fun onlyCompletedFilesInsideMediaCacheAreOpenable() {
        val root = Files.createTempDirectory("media-ui-test").toFile()
        try {
            val filesDir = File(root, "files").apply { mkdirs() }
            val mediaDir = File(filesDir, "telegram/media-cache").apply { mkdirs() }
            val cached = File(mediaDir, "abc.media").apply { writeBytes(byteArrayOf(1, 2, 3)) }
            val outside = File(filesDir, "other.txt").apply { writeText("private") }

            assertTrue(isConfinedMediaFile(filesDir, cached))
            assertFalse(isConfinedMediaFile(filesDir, outside))
            assertFalse(isConfinedMediaFile(filesDir, File(mediaDir, "missing.media")))
        } finally {
            root.deleteRecursively()
        }
    }

    @Test
    fun transferWireStateMapsProgressFailureAndCancellation() {
        assertEquals(
            MediaDownloadUiState.Queued,
            mediaDownloadStateFromWire(status = "queued"),
        )
        assertEquals(
            MediaDownloadUiState.Downloading(
                bytesTransferred = 25,
                totalBytes = 100,
            ),
            mediaDownloadStateFromWire(
                status = "downloading",
                bytesTransferred = 25,
                totalBytes = 100,
            ),
        )
        assertEquals(
            MediaDownloadUiState.Failed("network"),
            mediaDownloadStateFromWire(status = "failed", reason = "network"),
        )
        assertEquals(
            MediaDownloadUiState.Cancelled,
            mediaDownloadStateFromWire(status = "cancelled"),
        )
    }

    @Test
    fun missingOrEvictedDownloadedFileFallsBackToDownloadUx() {
        val attachment = MediaAttachmentUi(
            index = 0,
            kind = MediaKindUi.Photo,
            fileReference = "photo:1",
            sizeBytes = 100,
            mimeType = "image/jpeg",
            width = 640,
            height = 480,
            durationMs = null,
            filename = null,
            downloadState = MediaDownloadUiState.Downloaded(
                localPath = "/definitely/missing/media-file",
            ),
        )

        assertNull(attachment.downloadedFile)
        assertEquals(4f / 3f, attachment.aspectRatio)
    }

    @Test
    fun malformedDownloadedWireStateNeverCreatesFakeLocalPath() {
        assertEquals(
            MediaDownloadUiState.NotDownloaded,
            mediaDownloadStateFromWire(status = "downloaded", localPath = ""),
        )
        assertEquals(
            MediaDownloadUiState.NotDownloaded,
            mediaDownloadStateFromWire(status = "unexpected"),
        )
    }
}
