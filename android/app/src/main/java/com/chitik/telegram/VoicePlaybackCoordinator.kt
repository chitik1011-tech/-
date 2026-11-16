package com.chitik.telegram

import android.content.Context
import android.net.Uri
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Single player for a session. Never allocate a player from Compose recomposition. */
internal class VoicePlaybackCoordinator(
    context: Context,
    private val scope: CoroutineScope,
) {
    private val appContext = context.applicationContext
    private var player: ExoPlayer? = null
    private var progressJob: Job? = null
    private var epoch = 0L
    private val mutableState = MutableStateFlow(VoicePlaybackState())
    val state: StateFlow<VoicePlaybackState> = mutableState.asStateFlow()

    data class VoicePlaybackState(
        val mediaId: String? = null,
        val playing: Boolean = false,
        val positionMs: Long = 0L,
        val durationMs: Long? = null,
        val error: String? = null,
    )

    /** A new session or logout invalidates all ongoing playback and callbacks. */
    fun reset() {
        epoch++
        progressJob?.cancel()
        progressJob = null
        player?.release()
        player = null
        mutableState.value = VoicePlaybackState()
    }

    fun toggle(mediaId: String, file: File) {
        if (mediaId.isBlank() || !file.isFile || !file.canRead()) {
            mutableState.value = VoicePlaybackState(error = "Voice file is not available")
            return
        }
        val current = player
        if (mutableState.value.mediaId == mediaId && current != null) {
            if (current.isPlaying) current.pause() else current.play()
            return
        }
        reset()
        val expectedEpoch = epoch
        val newPlayer = ExoPlayer.Builder(appContext).build()
        player = newPlayer
        mutableState.value = VoicePlaybackState(mediaId = mediaId)
        newPlayer.setAudioAttributes(
            AudioAttributes.Builder()
                .setUsage(C.USAGE_MEDIA)
                .setContentType(C.AUDIO_CONTENT_TYPE_SPEECH)
                .build(),
            true,
        )
        newPlayer.setHandleAudioBecomingNoisy(true)
        newPlayer.addListener(object : Player.Listener {
            override fun onIsPlayingChanged(isPlaying: Boolean) {
                if (epoch != expectedEpoch) return
                mutableState.value = mutableState.value.copy(playing = isPlaying)
            }
            override fun onPlaybackStateChanged(playbackState: Int) {
                if (epoch != expectedEpoch) return
                if (playbackState == Player.STATE_ENDED) {
                    mutableState.value = mutableState.value.copy(playing = false, positionMs = 0)
                    newPlayer.seekTo(0)
                }
            }
            override fun onPlayerError(error: PlaybackException) {
                if (epoch != expectedEpoch) return
                mutableState.value = mutableState.value.copy(playing = false, error = "Audio playback failed")
            }
        })
        newPlayer.setMediaItem(
            MediaItem.Builder().setMediaId(mediaId).setUri(Uri.fromFile(file)).build(),
        )
        newPlayer.prepare()
        newPlayer.play()
        progressJob = scope.launch {
            while (isActive && epoch == expectedEpoch) {
                val duration = newPlayer.duration.takeIf { it > 0 && it != C.TIME_UNSET }
                mutableState.value = mutableState.value.copy(
                    positionMs = newPlayer.currentPosition.coerceAtLeast(0),
                    durationMs = duration,
                )
                delay(250)
            }
        }
    }
}
