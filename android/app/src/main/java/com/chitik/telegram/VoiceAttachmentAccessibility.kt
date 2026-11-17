package com.chitik.telegram

/** Screen-reader copy and a bounded progress value for one voice attachment state. */
internal data class VoicePlaybackAccessibility(
    val buttonLabel: String,
    val stateDescription: String,
    val progress: Float,
)

internal data class VoiceDownloadAccessibility(
    val description: String,
    val progress: Float?,
)

internal fun voicePlaybackAccessibility(
    playing: Boolean,
    positionMs: Long,
    durationMs: Long,
    error: String?,
): VoicePlaybackAccessibility {
    val duration = durationMs.coerceAtLeast(0L)
    val position = positionMs.coerceAtLeast(0L).coerceAtMost(duration)
    val fraction = if (duration == 0L) 0f else position.toFloat() / duration.toFloat()
    val progressText = if (duration > 0L) {
        "${formatMediaDuration(position) ?: "0:00"} из ${formatMediaDuration(duration) ?: "0:00"}"
    } else {
        formatMediaDuration(position) ?: "0:00"
    }
    val state = when {
        !error.isNullOrBlank() -> "Ошибка воспроизведения: ${error.trim()}. $progressText"
        playing -> "Воспроизводится, $progressText"
        else -> "На паузе, $progressText"
    }
    return VoicePlaybackAccessibility(
        buttonLabel = if (playing) "Пауза голосового сообщения" else "Воспроизвести голосовое сообщение",
        stateDescription = state,
        progress = fraction.coerceIn(0f, 1f),
    )
}

internal fun voiceDownloadAccessibility(
    state: MediaDownloadUiState,
): VoiceDownloadAccessibility = when (state) {
    MediaDownloadUiState.Queued -> VoiceDownloadAccessibility("Голосовое сообщение в очереди на загрузку", null)
    is MediaDownloadUiState.Downloading -> {
        val total = state.totalBytes?.takeIf { it > 0L }
        val transferred = state.bytesTransferred.coerceAtLeast(0L)
        val fraction = total?.let { (transferred.toDouble() / it).coerceIn(0.0, 1.0).toFloat() }
        val progress = total?.let {
            "${(transferred.toDouble() / it * 100.0).toInt()}% загружено"
        } ?: "Загружено ${formatMediaSize(transferred) ?: "данных"}"
        VoiceDownloadAccessibility("Загрузка голосового сообщения, $progress", fraction)
    }
    MediaDownloadUiState.NotDownloaded -> VoiceDownloadAccessibility("Голосовое сообщение не загружено", null)
    is MediaDownloadUiState.Failed -> VoiceDownloadAccessibility(
        "Ошибка загрузки голосового сообщения${state.reason?.takeIf(String::isNotBlank)?.let { ": $it" } ?: ""}. Повторить загрузку",
        null,
    )
    MediaDownloadUiState.Cancelled -> VoiceDownloadAccessibility("Загрузка отменена. Повторить загрузку голосового сообщения", null)
    is MediaDownloadUiState.Downloaded -> VoiceDownloadAccessibility("Файл голосового сообщения не найден в кэше. Загрузить снова", null)
}
