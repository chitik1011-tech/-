package com.chitik.telegram

import java.io.File
import java.util.Locale

enum class MediaKindUi {
    Photo,
    Video,
    Document,
    Audio,
    Voice,
    Animation,
    Sticker,
    VideoNote,
    Unknown,
}

sealed interface MediaDownloadUiState {
    data object NotDownloaded : MediaDownloadUiState
    data object Queued : MediaDownloadUiState
    data class Downloading(
        val bytesTransferred: Long,
        val totalBytes: Long?,
    ) : MediaDownloadUiState
    data class Downloaded(val localPath: String) : MediaDownloadUiState
    data class Failed(val reason: String?) : MediaDownloadUiState
    data object Cancelled : MediaDownloadUiState
}

data class MediaAttachmentUi(
    val index: Int,
    val kind: MediaKindUi,
    val fileReference: String,
    val sizeBytes: Long?,
    val mimeType: String?,
    val width: Int?,
    val height: Int?,
    val durationMs: Long?,
    val filename: String?,
    val downloadState: MediaDownloadUiState,
) {
    val aspectRatio: Float?
        get() {
            val safeWidth = width?.takeIf { it > 0 } ?: return null
            val safeHeight = height?.takeIf { it > 0 } ?: return null
            return safeWidth.toFloat() / safeHeight.toFloat()
        }

    val downloadedFile: File?
        get() = (downloadState as? MediaDownloadUiState.Downloaded)
            ?.localPath
            ?.let(::File)
            ?.takeIf(File::isFile)
}

internal fun mediaKindFromWire(value: String): MediaKindUi = when (value) {
    "photo" -> MediaKindUi.Photo
    "video" -> MediaKindUi.Video
    "document" -> MediaKindUi.Document
    "audio" -> MediaKindUi.Audio
    "voice" -> MediaKindUi.Voice
    "animation" -> MediaKindUi.Animation
    "sticker" -> MediaKindUi.Sticker
    "video_note" -> MediaKindUi.VideoNote
    else -> MediaKindUi.Unknown
}

internal fun mediaDownloadStateFromWire(
    status: String,
    bytesTransferred: Long = 0L,
    totalBytes: Long? = null,
    localPath: String? = null,
    reason: String? = null,
): MediaDownloadUiState = when (status) {
    "queued" -> MediaDownloadUiState.Queued
    "downloading" -> MediaDownloadUiState.Downloading(
        bytesTransferred = bytesTransferred.coerceAtLeast(0L),
        totalBytes = totalBytes?.takeIf { it > 0L },
    )
    "downloaded" -> localPath
        ?.takeIf { it.isNotBlank() }
        ?.let(MediaDownloadUiState::Downloaded)
        ?: MediaDownloadUiState.NotDownloaded
    "failed" -> MediaDownloadUiState.Failed(reason?.takeIf { it.isNotBlank() })
    "cancelled" -> MediaDownloadUiState.Cancelled
    else -> MediaDownloadUiState.NotDownloaded
}

internal fun sanitizeDisplayFilename(raw: String?): String {
    val basename = raw
        ?.replace('\\', '/')
        ?.substringAfterLast('/')
        ?.filterNot(Char::isISOControl)
        ?.trim()
        .orEmpty()
        .trim('.')
        .take(120)

    return basename.takeUnless { it.isBlank() || it == "." || it == ".." } ?: "file"
}

private val mimePattern = Regex("^[A-Za-z0-9!#$&^_.+-]+/[A-Za-z0-9!#$&^_.+-]+$")

internal fun resolveSafeMimeType(declared: String?, filename: String?): String {
    declared
        ?.trim()
        ?.takeIf(mimePattern::matches)
        ?.lowercase(Locale.ROOT)
        ?.let { return it }

    return when (sanitizeDisplayFilename(filename).substringAfterLast('.', "").lowercase(Locale.ROOT)) {
        "jpg", "jpeg" -> "image/jpeg"
        "png" -> "image/png"
        "bmp" -> "image/bmp"
        "webp" -> "image/webp"
        "gif" -> "image/gif"
        "mp4" -> "video/mp4"
        "webm" -> "video/webm"
        "mkv" -> "video/x-matroska"
        "mp3" -> "audio/mpeg"
        "ogg", "oga" -> "audio/ogg"
        "pdf" -> "application/pdf"
        "zip" -> "application/zip"
        "json" -> "application/json"
        "txt", "log", "md" -> "text/plain"
        else -> "application/octet-stream"
    }
}

/** True when an attachment can use the existing inline image preview and viewer. */
internal fun isInlineImageMedia(attachment: MediaAttachmentUi): Boolean =
    attachment.kind == MediaKindUi.Photo ||
        (attachment.kind == MediaKindUi.Document && resolveSafeMimeType(
            attachment.mimeType,
            attachment.filename,
        ) in INLINE_IMAGE_MIME_TYPES)

private val INLINE_IMAGE_MIME_TYPES = setOf(
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/bmp",
    "image/gif",
)

data class OpenIntentSpec(
    val action: String,
    val authority: String,
    val mimeType: String,
    val grantReadPermission: Boolean,
)

internal fun buildOpenIntentSpec(
    packageName: String,
    declaredMime: String?,
    filename: String?,
): OpenIntentSpec = OpenIntentSpec(
    action = "android.intent.action.VIEW",
    authority = "$packageName.media",
    mimeType = resolveSafeMimeType(declaredMime, filename),
    grantReadPermission = true,
)

internal fun isConfinedMediaFile(filesDir: File, candidate: File): Boolean = runCatching {
    val root = File(filesDir.canonicalFile, "telegram/media-cache").canonicalFile
    val file = candidate.canonicalFile
    val prefix = root.path.trimEnd(File.separatorChar) + File.separator
    file.isFile && file.path.startsWith(prefix)
}.getOrDefault(false)

internal fun formatMediaSize(bytes: Long?): String? {
    val value = bytes?.takeIf { it >= 0L } ?: return null
    if (value < 1024) return "$value B"
    val kib = value / 1024.0
    if (kib < 1024) return String.format(Locale.US, "%.1f KB", kib)
    val mib = kib / 1024.0
    if (mib < 1024) return String.format(Locale.US, "%.1f MB", mib)
    return String.format(Locale.US, "%.1f GB", mib / 1024.0)
}

internal fun formatMediaDuration(durationMs: Long?): String? {
    val totalSeconds = durationMs?.takeIf { it >= 0L }?.div(1000) ?: return null
    val hours = totalSeconds / 3600
    val minutes = (totalSeconds % 3600) / 60
    val seconds = totalSeconds % 60
    return if (hours > 0) {
        "%d:%02d:%02d".format(Locale.US, hours, minutes, seconds)
    } else {
        "%d:%02d".format(Locale.US, minutes, seconds)
    }
}
