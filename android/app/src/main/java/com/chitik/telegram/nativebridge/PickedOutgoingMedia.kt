package com.chitik.telegram.nativebridge

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import java.io.File
import java.io.IOException
import java.util.UUID

internal const val MAX_PICKED_MEDIA_BYTES: Long = 2L * 1024L * 1024L * 1024L

data class PickedOutgoingMedia(
    val kind: String,
    val path: String,
    val fileName: String,
    val mimeType: String,
    val sizeBytes: Long,
)

/**
 * Copies a Photo Picker or SAF content URI into app-private durable storage.
 * The Telegram core accepts only the resulting private path, never a content URI.
 */
internal fun importPickedMedia(context: Context, uri: Uri): PickedOutgoingMedia {
    val resolver = context.contentResolver
    val suppliedName = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
        ?.use { cursor ->
            if (cursor.moveToFirst()) {
                cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    .takeIf { it >= 0 }
                    ?.let(cursor::getString)
            } else {
                null
            }
        }
        ?.takeIf(String::isNotBlank)
        ?: uri.lastPathSegment?.substringAfterLast('/')?.takeIf(String::isNotBlank)
        ?: "attachment"

    val fileName = File(suppliedName).name
        .filterNot(Char::isISOControl)
        .take(255)
        .ifBlank { "attachment" }
    val extension = fileName.substringAfterLast('.', "")
        .takeIf { it.isNotBlank() && it.length <= 12 && it.all(Char::isLetterOrDigit) }
    val storageName = UUID.randomUUID().toString() + extension?.let { ".$it" }.orEmpty()
    val directory = File(context.applicationContext.filesDir, "outgoing-media")
    if (!directory.exists() && !directory.mkdirs()) {
        throw IOException("Could not prepare private media storage")
    }
    val destination = File(directory, storageName)
    val mimeType = resolver.getType(uri)
        ?.takeIf(String::isNotBlank)
        ?: extension?.let { MimeTypeMap.getSingleton().getMimeTypeFromExtension(it.lowercase()) }
        ?: "application/octet-stream"
    val kind = when {
        mimeType.startsWith("video/", ignoreCase = true) -> "video"
        mimeType.startsWith("image/", ignoreCase = true) &&
            mimeType.lowercase() !in setOf("image/gif", "image/svg+xml") -> "photo"
        else -> "document"
    }

    var copied = 0L
    try {
        val input = resolver.openInputStream(uri)
            ?: throw IOException("Could not read the selected media")
        input.use { source ->
            destination.outputStream().buffered().use { output ->
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    val count = source.read(buffer)
                    if (count < 0) break
                    copied += count
                    if (copied > MAX_PICKED_MEDIA_BYTES) {
                        throw IOException("Selected media exceeds the 2 GiB upload limit")
                    }
                    output.write(buffer, 0, count)
                }
            }
        }
        if (copied <= 0L) throw IOException("Selected media is empty")
    } catch (error: Throwable) {
        destination.delete()
        throw error
    }

    return PickedOutgoingMedia(
        kind = kind,
        path = destination.absolutePath,
        fileName = fileName,
        mimeType = mimeType,
        sizeBytes = copied,
    )
}

/** Deletes only an app-private media copy after its send reaches a terminal state. */
internal fun deletePickedMedia(context: Context, path: String) {
    runCatching {
        val directory = File(context.applicationContext.filesDir, "outgoing-media").canonicalFile
        val candidate = File(path).canonicalFile
        if (candidate.parentFile == directory && candidate.isFile) {
            candidate.delete()
        }
    }
}
