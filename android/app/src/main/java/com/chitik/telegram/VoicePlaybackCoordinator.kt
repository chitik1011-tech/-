package com.chitik.telegram

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
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

internal fun interface VoicePlayerFactory {
    fun create(): VoicePlayerPort
}

private class MainLooperVoicePlaybackScheduler : VoicePlaybackScheduler {
    private val handler = Handler(Looper.getMainLooper())
    override fun postDelayed(task: Runnable, delayMs: Long) { handler.postDelayed(task, delayMs) }
    override fun remove(task: Runnable) { handler.removeCallbacks(task) }
}

/**
 * Coordinates one active player across voice cards. A new player is created for each playback
 * epoch so callbacks from a released item cannot update its replacement. The owner should retain
 * one coordinator for the conversation/session and call [release] when permanently disposed.
 */
class VoicePlaybackCoordinator internal constructor(
    private val playerFactory: VoicePlayerFactory,
    private val scheduler: VoicePlaybackScheduler,
    private val publishIntervalMs: Long = 250L,
) {
    internal constructor(
        player: VoicePlayerPort,
        scheduler: VoicePlaybackScheduler,
        publishIntervalMs: Long = 250L,
    ) : this(VoicePlayerFactory { player }, scheduler, publishIntervalMs)

    private val observers = CopyOnWriteArraySet<(VoicePlaybackState) -> Unit>()
    @Volatile
    var state: VoicePlaybackState = VoicePlaybackState()
        private set
    private var playbackRequested = false
    private var resumeOnForeground = false
    private var hostStopped = false
    private var released = false
    private var playbackEpoch = 0L
    private var player: VoicePlayerPort? = null
    private var progressTick: Runnable? = null

    fun observe(observer: (VoicePlaybackState) -> Unit): () -> Unit {
        observers += observer
        observer(state)
        return { observers -= observer }
    }

    fun toggle(file: File) {
        if (released || hostStopped) return
        if (!file.isFile) {
            retirePlayer(stop = true)
            update(VoicePlaybackState(
                filePath = file.absolutePath,
                error = "Аудиофайл больше недоступен",
            ))
            return
        }
        val currentPlayer = player
        val currentEpoch = playbackEpoch
        if (state.filePath == file.absolutePath && currentPlayer != null &&
            (state.isPlaying || playbackRequested)
        ) {
            playbackRequested = false
            runPlayerAction(currentPlayer, currentEpoch) { it.pause() }
            return
        }
        if (state.filePath == file.absolutePath && currentPlayer != null) {
            playbackRequested = true
            runPlayerAction(currentPlayer, currentEpoch) { it.play(file) }
            return
        }
        startPlayback(file)
    }

    fun seekTo(positionMs: Long) {
        val currentPlayer = player ?: return
        if (released || state.filePath == null) return
        val bounded = positionMs.coerceIn(0L, state.durationMs.takeIf { it > 0L } ?: Long.MAX_VALUE)
        val currentEpoch = playbackEpoch
        runPlayerAction(currentPlayer, currentEpoch) {
            it.seekTo(bounded)
            update(state.copy(positionMs = bounded))
        }
    }

    /** Pause on background and only resume if playback was active before the transition. */
    fun onHostStop() {
        if (released || hostStopped) return
        hostStopped = true
        resumeOnForeground = state.isPlaying || playbackRequested
        if (resumeOnForeground) player?.let { current ->
            playbackRequested = false
            runPlayerAction(current, playbackEpoch) { it.pause() }
        }
    }

    fun onHostStart() {
        if (released || !hostStopped) return
        hostStopped = false
        if (resumeOnForeground) state.filePath?.let { path ->
            val file = File(path)
            if (file.isFile) player?.let { current ->
                playbackRequested = true
                runPlayerAction(current, playbackEpoch) { it.play(file) }
            } else {
                toggle(file)
            }
        }
        resumeOnForeground = false
    }

    fun release() {
        if (released) return
        released = true
        retirePlayer(stop = true)
        update(VoicePlaybackState())
        observers.clear()
    }

    private fun startPlayback(file: File) {
        retirePlayer(stop = true)
        update(VoicePlaybackState(filePath = file.absolutePath))
        val nextPlayer = try {
            playerFactory.create()
        } catch (_: RuntimeException) {
            update(state.copy(error = "Не удалось инициализировать аудиоплеер"))
            return
        }
        val epoch = playbackEpoch
        player = nextPlayer
        try {
            nextPlayer.setListener(listenerFor(nextPlayer, epoch))
            playbackRequested = true
            nextPlayer.play(file)
        } catch (_: RuntimeException) {
            failPlayback(nextPlayer, epoch, "Не удалось воспроизвести голосовое сообщение")
        }
    }

    private fun listenerFor(source: VoicePlayerPort, epoch: Long) = object : VoicePlayerPort.Listener {
        override fun onPlayingChanged(playing: Boolean) {
            if (!isCurrent(source, epoch)) return
            if (playing && hostStopped) {
                playbackRequested = false
                runPlayerAction(source, epoch) { it.pause() }
                return
            }
            update(state.copy(isPlaying = playing, error = null))
            if (playing) startProgressTick(source, epoch) else stopProgressTick()
        }

        override fun onCompleted() {
            if (!isCurrent(source, epoch)) return
            playbackRequested = false
            stopProgressTick()
            val durationMs = runCatching { source.durationMs() }
                .getOrNull()
                ?.coerceAtLeast(0L)
                ?: state.durationMs
            val completedPositionMs = durationMs.takeIf { it > 0L }
                ?: runCatching { source.currentPositionMs() }
                    .getOrNull()
                    ?.coerceAtLeast(0L)
                ?: state.positionMs
            update(
                state.copy(
                    isPlaying = false,
                    positionMs = completedPositionMs,
                    durationMs = durationMs,
                ),
            )
        }

        override fun onError(message: String) {
            failPlayback(source, epoch, message)
        }
    }

    private fun startProgressTick(source: VoicePlayerPort, epoch: Long) {
        stopProgressTick()
        val tick = object : Runnable {
            override fun run() {
                if (!isCurrent(source, epoch) || !state.isPlaying) return
                runPlayerAction(source, epoch) { current ->
                    update(state.copy(
                        positionMs = current.currentPositionMs().coerceAtLeast(0L),
                        durationMs = current.durationMs().coerceAtLeast(0L),
                    ))
                }
                if (isCurrent(source, epoch) && state.isPlaying) {
                    scheduler.postDelayed(this, publishIntervalMs)
                }
            }
        }
        progressTick = tick
        scheduler.postDelayed(tick, publishIntervalMs)
    }

    private fun stopProgressTick() {
        progressTick?.let(scheduler::remove)
        progressTick = null
    }

    private fun runPlayerAction(
        source: VoicePlayerPort,
        epoch: Long,
        action: (VoicePlayerPort) -> Unit,
    ) {
        if (!isCurrent(source, epoch)) return
        try {
            action(source)
        } catch (_: RuntimeException) {
            failPlayback(source, epoch, "Не удалось воспроизвести голосовое сообщение")
        }
    }

    private fun failPlayback(source: VoicePlayerPort, epoch: Long, message: String) {
        if (!isCurrent(source, epoch)) return
        val failedState = state.copy(isPlaying = false, error = message)
        retirePlayer(stop = false)
        update(failedState)
    }

    private fun isCurrent(source: VoicePlayerPort, epoch: Long): Boolean =
        !released && playbackEpoch == epoch && player === source

    private fun retirePlayer(stop: Boolean) {
        playbackEpoch++
        val previous = player
        player = null
        playbackRequested = false
        stopProgressTick()
        if (previous != null) {
            runCatching { previous.setListener(null) }
            if (stop) runCatching { previous.stop() }
            runCatching { previous.release() }
        }
    }

    private fun update(next: VoicePlaybackState) {
        state = next
        observers.forEach { it(next) }
    }

    companion object {
        /** Creates a coordinator backed by at most one active Media3 player. */
        fun create(context: Context): VoicePlaybackCoordinator =
            VoicePlaybackCoordinator(
                VoicePlayerFactory { Media3VoicePlayer(context.applicationContext) },
                MainLooperVoicePlaybackScheduler(),
            )
    }
}

private class Media3VoicePlayer(context: Context) : VoicePlayerPort {
    private val player = ExoPlayer.Builder(context)
        .setHandleAudioBecomingNoisy(true)
        .build()
    private var listener: VoicePlayerPort.Listener? = null
    private var released = false
    private val mediaListener = object : Player.Listener {
        override fun onIsPlayingChanged(isPlaying: Boolean) {
            listener?.onPlayingChanged(isPlaying)
        }

        override fun onPlaybackStateChanged(playbackState: Int) {
            if (playbackState == Player.STATE_ENDED) listener?.onCompleted()
        }

        override fun onPlayerError(error: PlaybackException) {
            listener?.onError("Не удалось воспроизвести голосовое сообщение")
        }
    }

    init {
        try {
            player.setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(C.USAGE_MEDIA)
                    .setContentType(C.AUDIO_CONTENT_TYPE_SPEECH)
                    .build(),
                true,
            )
            player.addListener(mediaListener)
        } catch (error: RuntimeException) {
            runCatching { player.release() }
            throw error
        }
    }

    override fun play(file: File) {
        check(file.isFile) { "Voice file is not available" }
        val sameFile = player.currentMediaItem?.localConfiguration?.uri == Uri.fromFile(file)
        if (!sameFile) {
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
    override fun release() {
        if (released) return
        released = true
        listener = null
        runCatching { player.removeListener(mediaListener) }
        player.release()
    }
}
