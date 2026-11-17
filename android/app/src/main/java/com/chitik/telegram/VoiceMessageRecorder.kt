package com.chitik.telegram

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.media.MediaRecorder
import android.os.Build
import android.os.SystemClock
import androidx.core.content.ContextCompat
import java.io.File
import java.util.UUID

/** Metadata returned for a finalized Ogg/Opus recording, ready for the reliable media contract. */
data class RecordedVoiceMessage internal constructor(
    val file: File,
    val durationMs: Long,
    val sizeBytes: Long,
) {
    val path: String get() = file.absolutePath
    val fileName: String get() = file.name
    val mimeType: String get() = "audio/ogg"
}

enum class VoiceRecorderFailure {
    /** Ogg/Opus recording requires Android 10 (API 29) or newer. */
    UNSUPPORTED_ANDROID_VERSION,
    /** The caller must request RECORD_AUDIO before starting. */
    MICROPHONE_PERMISSION_REQUIRED,
    /** A recording is already active on this recorder. */
    ALREADY_RECORDING,
    /** No recording is active to stop. */
    NOT_RECORDING,
    /** This recorder has been closed with its owning lifecycle. */
    CLOSED,
    /** The device could not prepare or start a genuine Ogg/Opus encoder. */
    OGG_OPUS_UNAVAILABLE,
    /** The recording could not be finalized or its output was empty. */
    FINALIZATION_FAILED,
    /** Telegram voice metadata accepts a duration of 1 second through 48 hours. */
    DURATION_OUT_OF_RANGE,
}

sealed interface VoiceRecorderResult {
    data object RecordingStarted : VoiceRecorderResult
    data class RecordingReady(val voice: RecordedVoiceMessage) : VoiceRecorderResult
    data class Failed(val reason: VoiceRecorderFailure) : VoiceRecorderResult
}

/**
 * Owns one app-private Ogg/Opus microphone recording at a time.
 *
 * The caller requests microphone permission before [start], observes its result, and calls
 * [close] when its owning lifecycle ends. Call [stop] to finalize a recording or [cancel] to
 * discard it. Completed files remain in the app-private files directory for the caller to hand
 * to `send_media_reliable`.
 */
class VoiceMessageRecorder(context: Context) : AutoCloseable {
    private val appContext = context.applicationContext
    private val controller = VoiceRecordingController(
        outputDirectory = File(appContext.filesDir, "voice-recordings"),
        apiLevel = Build.VERSION.SDK_INT,
        hasMicrophonePermission = {
            ContextCompat.checkSelfPermission(
                appContext,
                Manifest.permission.RECORD_AUDIO,
            ) == PackageManager.PERMISSION_GRANTED
        },
        engineFactory = { AndroidOggOpusRecorder() },
        elapsedRealtimeMs = SystemClock::elapsedRealtime,
    )

    @Synchronized
    fun start(): VoiceRecorderResult = controller.start()

    @Synchronized
    fun stop(): VoiceRecorderResult = controller.stop()

    /** Discard an active recording and remove its partial file. Safe to call more than once. */
    @Synchronized
    fun cancel(): Boolean = controller.cancel()

    /** Cancels any in-progress recording and releases microphone resources. */
    @Synchronized
    override fun close() = controller.close()
}

/** Injectable, Android-independent lifecycle/state machine used by the recorder and JVM tests. */
internal class VoiceRecordingController(
    private val outputDirectory: File,
    private val apiLevel: Int,
    private val hasMicrophonePermission: () -> Boolean,
    private val engineFactory: () -> VoiceRecordingEngine,
    private val elapsedRealtimeMs: () -> Long,
    private val fileName: () -> String = { "voice-${UUID.randomUUID()}.ogg" },
) : AutoCloseable {
    private data class ActiveRecording(
        val file: File,
        val engine: VoiceRecordingEngine,
        val startedAtMs: Long,
    )

    private var active: ActiveRecording? = null
    private var closed = false

    @Synchronized
    fun start(): VoiceRecorderResult {
        if (closed) return VoiceRecorderResult.Failed(VoiceRecorderFailure.CLOSED)
        if (active != null) return VoiceRecorderResult.Failed(VoiceRecorderFailure.ALREADY_RECORDING)
        if (apiLevel < MIN_OGG_OPUS_API) {
            return VoiceRecorderResult.Failed(VoiceRecorderFailure.UNSUPPORTED_ANDROID_VERSION)
        }
        if (!hasMicrophonePermission()) {
            return VoiceRecorderResult.Failed(VoiceRecorderFailure.MICROPHONE_PERMISSION_REQUIRED)
        }

        val file = try {
            if (!outputDirectory.exists() && !outputDirectory.mkdirs()) {
                return VoiceRecorderResult.Failed(VoiceRecorderFailure.OGG_OPUS_UNAVAILABLE)
            }
            File(outputDirectory, fileName()).also { target ->
                require(target.parentFile?.canonicalFile == outputDirectory.canonicalFile)
                if (!target.createNewFile()) error("recording file already exists")
            }
        } catch (_: Exception) {
            return VoiceRecorderResult.Failed(VoiceRecorderFailure.OGG_OPUS_UNAVAILABLE)
        }

        val engine = try {
            engineFactory()
        } catch (_: Exception) {
            file.delete()
            return VoiceRecorderResult.Failed(VoiceRecorderFailure.OGG_OPUS_UNAVAILABLE)
        }
        try {
            engine.prepareAndStart(file)
            active = ActiveRecording(file, engine, elapsedRealtimeMs())
            return VoiceRecorderResult.RecordingStarted
        } catch (_: Exception) {
            runCatching { engine.release() }
            file.delete()
            return VoiceRecorderResult.Failed(VoiceRecorderFailure.OGG_OPUS_UNAVAILABLE)
        }
    }

    @Synchronized
    fun stop(): VoiceRecorderResult {
        val recording = active ?: return VoiceRecorderResult.Failed(VoiceRecorderFailure.NOT_RECORDING)
        active = null
        val stoppedAtMs = elapsedRealtimeMs()
        val finalized = try {
            recording.engine.stop()
            true
        } catch (_: Exception) {
            false
        } finally {
            runCatching { recording.engine.release() }
        }
        if (!finalized) {
            recording.file.delete()
            return VoiceRecorderResult.Failed(VoiceRecorderFailure.FINALIZATION_FAILED)
        }

        val durationMs = (stoppedAtMs - recording.startedAtMs).coerceAtLeast(0L)
        if (durationMs !in MIN_DURATION_MS..MAX_DURATION_MS) {
            recording.file.delete()
            return VoiceRecorderResult.Failed(VoiceRecorderFailure.DURATION_OUT_OF_RANGE)
        }
        val sizeBytes = recording.file.length()
        if (!recording.file.isFile || sizeBytes <= 0L) {
            recording.file.delete()
            return VoiceRecorderResult.Failed(VoiceRecorderFailure.FINALIZATION_FAILED)
        }
        return VoiceRecorderResult.RecordingReady(
            RecordedVoiceMessage(recording.file, durationMs, sizeBytes),
        )
    }

    @Synchronized
    fun cancel(): Boolean {
        val recording = active ?: return false
        active = null
        runCatching { recording.engine.release() }
        recording.file.delete()
        return true
    }

    @Synchronized
    override fun close() {
        if (closed) return
        cancel()
        closed = true
    }

    private companion object {
        const val MIN_OGG_OPUS_API = 29
        const val MIN_DURATION_MS = 1_000L
        const val MAX_DURATION_MS = 48L * 60L * 60L * 1_000L
    }
}

internal interface VoiceRecordingEngine {
    /** Must configure Ogg + Opus, prepare, and start; any failure rejects this recording. */
    fun prepareAndStart(output: File)
    fun stop()
    fun release()
}

/** Uses the platform encoder directly; no alternate codec/container is ever written as `.ogg`. */
private class AndroidOggOpusRecorder : VoiceRecordingEngine {
    private var recorder: MediaRecorder? = null

    override fun prepareAndStart(output: File) {
        check(Build.VERSION.SDK_INT >= 29) { "Ogg/Opus recording requires API 29" }
        val instance = MediaRecorder()
        recorder = instance
        instance.setAudioSource(MediaRecorder.AudioSource.MIC)
        instance.setOutputFormat(MediaRecorder.OutputFormat.OGG)
        instance.setAudioEncoder(MediaRecorder.AudioEncoder.OPUS)
        instance.setAudioSamplingRate(48_000)
        instance.setAudioEncodingBitRate(32_000)
        instance.setOutputFile(output)
        instance.prepare()
        instance.start()
    }

    override fun stop() {
        checkNotNull(recorder).stop()
    }

    override fun release() {
        recorder?.release()
        recorder = null
    }
}
