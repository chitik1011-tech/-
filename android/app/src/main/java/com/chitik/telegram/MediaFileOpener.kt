package com.chitik.telegram

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Context
import android.content.Intent
import androidx.core.content.FileProvider
import java.io.File

sealed interface OpenMediaResult {
    data object Opened : OpenMediaResult
    data object MissingFile : OpenMediaResult
    data object UnsafePath : OpenMediaResult
    data object NoHandler : OpenMediaResult
    data class Failed(val message: String) : OpenMediaResult
}

object MediaFileOpener {
    fun open(context: Context, attachment: MediaAttachmentUi): OpenMediaResult {
        val downloaded = attachment.downloadState as? MediaDownloadUiState.Downloaded
            ?: return OpenMediaResult.MissingFile
        val file = File(downloaded.localPath)
        if (!file.isFile) return OpenMediaResult.MissingFile
        if (!isConfinedMediaFile(context.filesDir, file)) return OpenMediaResult.UnsafePath

        val spec = buildOpenIntentSpec(
            packageName = context.packageName,
            declaredMime = attachment.mimeType,
            filename = attachment.filename,
        )

        return try {
            val uri = FileProvider.getUriForFile(context, spec.authority, file)
            val intent = Intent(spec.action)
                .setDataAndType(uri, spec.mimeType)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                .putExtra(Intent.EXTRA_TITLE, sanitizeDisplayFilename(attachment.filename))
                .apply {
                    clipData = ClipData.newRawUri("media", uri)
                }

            context.startActivity(intent)
            OpenMediaResult.Opened
        } catch (_: ActivityNotFoundException) {
            OpenMediaResult.NoHandler
        } catch (error: Exception) {
            OpenMediaResult.Failed(error.message ?: error::class.java.simpleName)
        }
    }
}
