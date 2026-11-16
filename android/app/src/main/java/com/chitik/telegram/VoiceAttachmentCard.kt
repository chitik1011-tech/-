package com.chitik.telegram

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import java.io.File

/** Telegram packs amplitude samples as successive 5-bit little-endian values. */
internal fun decodeVoiceWaveform(bytes: List<Int>?, maxSamples: Int = 40): List<Int> {
    if (bytes.isNullOrEmpty() || bytes.size > 4096 || bytes.any { it !in 0..255 }) return emptyList()
    val count = (bytes.size * 8 / 5).coerceAtMost(maxSamples.coerceAtLeast(0))
    return (0 until count).map { sample ->
        val offset = sample * 5
        val byteIndex = offset / 8
        val shift = offset % 8
        val low = bytes[byteIndex] ushr shift
        val high = bytes.getOrElse(byteIndex + 1) { 0 } shl (8 - shift)
        (low or high) and 31
    }
}

/** Voice-only bubble. Uses the existing attachment download state and transfer commands. */
@Composable
fun VoiceAttachmentCard(
    attachment: MediaAttachmentUi,
    mediaId: String,
    playback: VoicePlaybackCoordinator.VoicePlaybackState,
    onTogglePlayback: () -> Unit,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        val isCurrent = playback.mediaId == mediaId
        val isPlaying = isCurrent && playback.playing
        val file = (attachment.downloadState as? MediaDownloadUiState.Downloaded)
            ?.localPath?.let(::File)
        val playable = file?.isFile == true
        Row(verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            when (val transfer = attachment.downloadState) {
                is MediaDownloadUiState.Downloaded -> {
                    if (playable) {
                        Button(onClick = onTogglePlayback) {
                            Text(if (isPlaying) "Pause" else "Play")
                        }
                    } else {
                        Button(onClick = { onDownload(true) }) { Text("Download again") }
                    }
                }
                MediaDownloadUiState.NotDownloaded -> {
                    Button(onClick = { onDownload(false) }) { Text("Download voice") }
                }
                MediaDownloadUiState.Queued -> {
                    Text("Queued")
                    TextButton(onClick = onCancel) { Text("Cancel") }
                }
                is MediaDownloadUiState.Downloading -> {
                    Text("Downloading")
                    TextButton(onClick = onCancel) { Text("Cancel") }
                }
                is MediaDownloadUiState.Failed, MediaDownloadUiState.Cancelled -> {
                    Button(onClick = { onDownload(true) }) { Text("Retry") }
                }
            }
            val length = if (isCurrent) playback.durationMs ?: attachment.durationMs
                else attachment.durationMs
            Text(MediaDurationFormatter.format(length))
        }
        val bars = decodeVoiceWaveform(attachment.voiceWaveform).ifEmpty { List(24) { 12 } }
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            bars.forEach { sample ->
                val height = (5 + sample * 19 / 31).dp
                Box(
                    Modifier.width(2.dp).height(height)
                        .background(MaterialTheme.colorScheme.primary),
                )
            }
        }
        if (isCurrent && playback.error != null) {
            Text(playback.error, color = MaterialTheme.colorScheme.error)
        }
        if (isCurrent && (playback.durationMs ?: 0L) > 0L) {
            val fraction = (playback.positionMs.toFloat() / playback.durationMs!!.toFloat())
                .coerceIn(0f, 1f)
            LinearProgressIndicator(progress = { fraction }, modifier = Modifier.fillMaxWidth())
        }
        val transfer = attachment.downloadState
        if (transfer is MediaDownloadUiState.Downloading) {
            val total = transfer.totalBytes
            if (total != null && total > 0) {
                LinearProgressIndicator(
                    progress = { (transfer.bytesTransferred.toFloat() / total).coerceIn(0f, 1f) },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}
