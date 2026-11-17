package com.chitik.telegram

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import java.io.File
import java.util.concurrent.CopyOnWriteArraySet

/** State for the single active voice message managed by a screen/session owner. */
data class VoicePlaybackState(
    val filePath: String? = null,
    val isPlaying: Boolean = false,
    val positionMs: Long = 0L,
    val durationMs: Long = 0L,
    val error: String? = null,
)

/** Small player boundary keeps ownership, lifecycle and exclusivity behavior testable. */
interface VoicePlayerPort {
    fun play(file: File)
    fun pause()
    fun stop()
    fun seekTo(positionMs: Long)
    fun currentPositionMs(): Long
    fun durationMs(): Long
    fun setListener(listener: Listener?)
    fun release()

    interface Listener {
        fun onPlayingChanged(playing: Boolean)
        fun onCompleted()
        fun onError(message: String)
    }
}

internal interface VoicePlaybackScheduler {
    fun postDelayed(task: Runnable, delayMs: Long)
    fun remove(task: Runnable)
}

private class MainLooperVoicePlaybackScheduler : VoicePlaybackScheduler {
    private val handler = Handler(Looper.getMainLooper())
    override fun postDelayed(task: Runnable, delayMs: Long) { handler.postDelayed(task, delayMs) }
    override fun remove(task: Runnable) { handler.removeCallbacks(task) }
}

/**
 * Coordinates one player across voice cards. The owner should retain one instance for the
 * conversation/session and call [release] when that owner is permanently disposed.
 */
class VoicePlaybackCoordinator internal constructor(
    private val player: VoicePlayerPort,
    private val scheduler: VoicePlaybackScheduler,
    private val publishIntervalMs: Long = 250L,
) {
    private val observers = CopyOnWriteArraySet<(VoicePlaybackState) -> Unit>()
    @Volatile
    var state: VoicePlaybackState = VoicePlaybackState()
        private set
    private var resumeOnForeground = false
    private var hostStopped = false
    private var released = false

    private val playerListener = object : VoicePlayerPort.Listener {
        override fun onPlayingChanged(playing: Boolean) {
            update(state.copy(isPlaying = playing, error = null))
            if (playing) scheduler.postDelayed(progressTick, publishIntervalMs) else scheduler.remove(progressTick)
        }

        override fun onCompleted() {
            scheduler.remove(progressTick)
            update(state.copy(isPlaying = false, positionMs = state.durationMs))
        }

        override fun onError(message: String) {
            scheduler.remove(progressTick)
            update(state.copy(isPlaying = false, error = message))
        }
    }

    private val progressTick = object : Runnable {
        override fun run() {
            if (released || !state.isPlaying) return
            update(state.copy(
                positionMs = player.currentPositionMs().coerceAtLeast(0L),
                durationMs = player.durationMs().coerceAtLeast(0L),
            ))
            scheduler.postDelayed(this, publishIntervalMs)
        }
    }

    init { player.setListener(playerListener) }

    fun observe(observer: (VoicePlaybackState) -> Unit): () -> Unit {
        observers += observer
        observer(state)
        return { observers -= observer }
    }

    fun toggle(file: File) {
        if (released || !file.isFile) return
        if (state.filePath == file.absolutePath && state.isPlaying) {
            player.pause()
            return
        }
        if (state.filePath == file.absolutePath) {
            player.play(file)
            return
        }
        val replacingActiveFile = state.filePath != null
        scheduler.remove(progressTick)
        update(VoicePlaybackState(filePath = file.absolutePath))
        if (replacingActiveFile) player.stop()
        player.play(file)
    }

    fun seekTo(positionMs: Long) {
        if (released || state.filePath == null) return
        val bounded = positionMs.coerceIn(0L, state.durationMs.takeIf { it > 0L } ?: Long.MAX_VALUE)
        player.seekTo(bounded)
        update(state.copy(positionMs = bounded))
    }

    /** Pause on background and only resume if playback was active before the transition. */
    fun onHostStop() {
        if (released || hostStopped) return
        hostStopped = true
        resumeOnForeground = state.isPlaying
        if (resumeOnForeground) player.pause()
    }

    fun onHostStart() {
        if (released || !hostStopped) return
        hostStopped = false
        if (resumeOnForeground) state.filePath?.let { path ->
            val file = File(path)
            if (file.isFile) player.play(file)
        }
        resumeOnForeground = false
    }

    fun release() {
        if (released) return
        released = true
        scheduler.remove(progressTick)
        player.setListener(null)
        player.release()
        update(VoicePlaybackState())
        observers.clear()
    }

    private fun update(next: VoicePlaybackState) {
        state = next
        observers.forEach { it(next) }
    }

    companion object {
        /** Creates a coordinator backed by one Media3 player. */
        fun create(context: Context): VoicePlaybackCoordinator =
            VoicePlaybackCoordinator(Media3VoicePlayer(context.applicationContext), MainLooperVoicePlaybackScheduler())
    }
}

private class Media3VoicePlayer(context: Context) : VoicePlayerPort {
    private val player = ExoPlayer.Builder(context).build()
    private var listener: VoicePlayerPort.Listener? = null
    private var failed = false
    private val mediaListener = object : Player.Listener {
        override fun onIsPlayingChanged(isPlaying: Boolean) {
            listener?.onPlayingChanged(isPlaying)
        }

        override fun onPlaybackStateChanged(playbackState: Int) {
            if (playbackState == Player.STATE_ENDED) listener?.onCompleted()
        }

        override fun onPlayerError(error: PlaybackException) {
            failed = true
            listener?.onError("Не удалось воспроизвести голосовое сообщение")
        }
    }

    init { player.addListener(mediaListener) }

    override fun play(file: File) {
        if (!file.isFile) return
        val sameFile = player.currentMediaItem?.localConfiguration?.uri == Uri.fromFile(file)
        if (!sameFile || failed) {
            failed = false
            player.setMediaItem(MediaItem.fromUri(Uri.fromFile(file)))
            player.prepare()
        }
        if (sameFile && player.playbackState == Player.STATE_ENDED) player.seekTo(0L)
        player.play()
    }
    override fun pause() = player.pause()
    override fun stop() = player.stop()
    override fun seekTo(positionMs: Long) = player.seekTo(positionMs)
    override fun currentPositionMs(): Long = player.currentPosition.coerceAtLeast(0L)
    override fun durationMs(): Long = player.duration.takeIf { it >= 0L } ?: 0L
    override fun setListener(listener: VoicePlayerPort.Listener?) { this.listener = listener }
    override fun release() { player.removeListener(mediaListener); player.release() }
}
