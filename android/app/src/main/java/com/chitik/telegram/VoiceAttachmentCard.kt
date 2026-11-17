package com.chitik.telegram

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/**
 * Renders an incoming voice attachment from the existing media cache. The caller owns and
 * shares [coordinator] across cards so only one voice player exists in the conversation.
 */
@Composable
fun VoiceAttachmentCard(
    attachment: MediaAttachmentUi,
    coordinator: VoicePlaybackCoordinator,
    onDownload: (retry: Boolean) -> Unit,
    onCancelDownload: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var playback by remember(coordinator) { mutableStateOf(coordinator.state) }

    DisposableEffect(coordinator) {
        val removeObserver = coordinator.observe { playback = it }
        onDispose {
            removeObserver()
        }
    }

    val file = attachment.downloadedFile
    val active = file != null && playback.filePath == file.absolutePath
    val playing = active && playback.isPlaying
    val bars = remember(attachment.voiceWaveform) {
        formatVoiceWaveform(attachment.voiceWaveform.orEmpty())
    }
    val color = MaterialTheme.colorScheme.primary
    val playbackAccessibility = voicePlaybackAccessibility(
        playing = playing,
        positionMs = if (active) playback.positionMs else 0L,
        durationMs = attachment.durationMs ?: if (active) playback.durationMs else 0L,
        error = if (active) playback.error else null,
    )

    Surface(
        modifier = modifier.defaultMinSize(minWidth = 224.dp),
        shape = RoundedCornerShape(18.dp),
        color = MaterialTheme.colorScheme.secondaryContainer,
        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (file != null) {
                IconButton(onClick = { coordinator.toggle(file) }) {
                    Icon(
                        imageVector = if (playing) Icons.Default.Pause else Icons.Default.PlayArrow,
                        contentDescription = playbackAccessibility.buttonLabel,
                    )
                }
                Row(
                    modifier = Modifier
                        .weight(1f)
                        .height(34.dp)
                        .semantics {
                            contentDescription = "Голосовое сообщение"
                            stateDescription = playbackAccessibility.stateDescription
                            progressBarRangeInfo = ProgressBarRangeInfo(
                                playbackAccessibility.progress,
                                0f..1f,
                            )
                        },
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(1.dp),
                ) {
                    if (bars.isEmpty()) {
                        Text("Голосовое сообщение", style = MaterialTheme.typography.bodySmall)
                    } else {
                        bars.forEachIndexed { index, height ->
                            val progress = if (active && playback.durationMs > 0L) {
                                playback.positionMs.toFloat() / playback.durationMs
                            } else 0f
                            Box(
                                modifier = Modifier
                                    .width(2.dp)
                                    .height(height.dp)
                                    .clip(CircleShape)
                                    .clearAndSetSemantics { }
                                    .background(
                                        if (index.toFloat() / bars.size < progress) color
                                        else MaterialTheme.colorScheme.onSecondaryContainer.copy(alpha = 0.45f),
                                    ),
                            )
                        }
                    }
                }
                ColumnVoiceTime(
                    positionMs = if (active) playback.positionMs else 0L,
                    durationMs = attachment.durationMs ?: if (active) playback.durationMs else 0L,
                    error = if (active) playback.error else null,
                )
            } else {
                DownloadStatus(attachment.downloadState, onDownload, onCancelDownload)
            }
        }
    }
}

@Composable
private fun ColumnVoiceTime(positionMs: Long, durationMs: Long, error: String?) {
    androidx.compose.foundation.layout.Column(horizontalAlignment = Alignment.End) {
        Text(
            text = error ?: formatMediaDuration(positionMs) ?: "0:00",
            style = MaterialTheme.typography.labelSmall,
            maxLines = 1,
            overflow = TextOverflow.Clip,
        )
        if (error == null && durationMs > 0L) {
            Text(formatMediaDuration(durationMs) ?: "0:00", style = MaterialTheme.typography.labelSmall)
        }
    }
}

@Composable
private fun DownloadStatus(
    state: MediaDownloadUiState,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
) {
    when (state) {
        MediaDownloadUiState.Queued -> Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            CircularProgressIndicator(modifier = Modifier.width(20.dp), strokeWidth = 2.dp)
            Text(
                "В очереди",
                modifier = Modifier.semantics {
                    contentDescription = voiceDownloadAccessibility(state).description
                },
                style = MaterialTheme.typography.bodySmall,
            )
            TextButton(onClick = onCancel) { Text("Отмена") }
        }
        is MediaDownloadUiState.Downloading -> Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            val fraction = state.totalBytes
                ?.takeIf { it > 0L }
                ?.let { (state.bytesTransferred.toDouble() / it).coerceIn(0.0, 1.0).toFloat() }
            if (fraction != null) {
                val accessibility = voiceDownloadAccessibility(state)
                LinearProgressIndicator(
                    progress = { fraction },
                    modifier = Modifier
                        .width(72.dp)
                        .semantics {
                            contentDescription = accessibility.description
                            stateDescription = accessibility.description
                            progressBarRangeInfo = ProgressBarRangeInfo(fraction, 0f..1f)
                        },
                )
                Text(
                    "${(fraction * 100).toInt()}%",
                    modifier = Modifier.clearAndSetSemantics { },
                    style = MaterialTheme.typography.labelSmall,
                )
            } else {
                CircularProgressIndicator(modifier = Modifier.width(20.dp), strokeWidth = 2.dp)
                Text(formatMediaSize(state.bytesTransferred) ?: "Загрузка…", style = MaterialTheme.typography.bodySmall)
            }
            TextButton(onClick = onCancel) { Text("Отмена") }
        }
        MediaDownloadUiState.NotDownloaded -> DownloadAction(
            label = "Загрузить голосовое сообщение",
            accessibilityDescription = voiceDownloadAccessibility(state).description,
            onClick = { onDownload(false) },
        )
        is MediaDownloadUiState.Failed -> DownloadAction(
            label = state.reason ?: "Не удалось скачать · повторить",
            accessibilityDescription = voiceDownloadAccessibility(state).description,
            onClick = { onDownload(true) },
        )
        MediaDownloadUiState.Cancelled -> DownloadAction(
            label = "Загрузка отменена · повторить",
            accessibilityDescription = voiceDownloadAccessibility(state).description,
            onClick = { onDownload(true) },
        )
        is MediaDownloadUiState.Downloaded -> DownloadAction(
            label = "Файл удалён из кэша · скачать заново",
            accessibilityDescription = voiceDownloadAccessibility(state).description,
            onClick = { onDownload(true) },
        )
    }
}

@Composable
private fun DownloadAction(
    label: String,
    accessibilityDescription: String,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .clip(RoundedCornerShape(8.dp))
            .padding(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TextButton(
            onClick = onClick,
            modifier = Modifier.semantics {
                contentDescription = accessibilityDescription
            },
        ) {
            Text(label, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}

/** Convert Telegram's amplitude samples into a bounded set of waveform bar heights. */
internal fun formatVoiceWaveform(samples: List<Int>, maxBars: Int = 48): List<Int> {
    if (samples.isEmpty() || maxBars <= 0) return emptyList()
    val bucketCount = minOf(maxBars, samples.size, 48)
    val stride = samples.size.toFloat() / bucketCount
    return List(bucketCount) { bucket ->
        val start = (bucket * stride).toInt().coerceAtMost(samples.lastIndex)
        val end = ((bucket + 1) * stride).toInt().coerceAtLeast(start + 1).coerceAtMost(samples.size)
        val peak = samples.subList(start, end).maxOf { it.coerceIn(0, 31) }
        (4 + (peak * 20 / 31)).coerceIn(4, 24)
    }
}
