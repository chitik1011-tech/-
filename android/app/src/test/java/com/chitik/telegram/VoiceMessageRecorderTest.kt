package com.chitik.telegram

import java.io.File
import java.io.IOException
import java.nio.file.Files
import com.chitik.telegram.nativebridge.decodeTelegramVoiceWaveform
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VoiceMessageRecorderTest {
    @Test
    fun api26RejectsOggOpusWithoutCreatingAFileOrStartingAnEngine() {
        val directory = Files.createTempDirectory("voice-api26").toFile()
        var created = false
        val recorder = controller(directory, apiLevel = 26, engineFactory = {
            created = true
            FakeEngine()
        })
        try {
            assertEquals(
                VoiceRecorderResult.Failed(VoiceRecorderFailure.UNSUPPORTED_ANDROID_VERSION),
                recorder.start(),
            )
            assertFalse(created)
            assertTrue(directory.listFiles().orEmpty().isEmpty())
        } finally {
            recorder.close()
            directory.deleteRecursively()
        }
    }

    @Test
    fun permissionIsRequiredBeforeTheMicrophoneCanStart() {
        val directory = Files.createTempDirectory("voice-permission").toFile()
        var created = false
        val recorder = controller(directory, hasPermission = false, engineFactory = {
            created = true
            FakeEngine()
        })
        try {
            assertEquals(
                VoiceRecorderResult.Failed(VoiceRecorderFailure.MICROPHONE_PERMISSION_REQUIRED),
                recorder.start(),
            )
            assertFalse(created)
            assertTrue(directory.listFiles().orEmpty().isEmpty())
        } finally {
            recorder.close()
            directory.deleteRecursively()
        }
    }

    @Test
    fun successfulOggOpusRecordingReturnsPrivatePathAndElapsedDuration() {
        val directory = Files.createTempDirectory("voice-ready").toFile()
        var now = 10_000L
        val engine = FakeEngine(amplitudes = listOf(0, 10, 20, 30))
        val recorder = controller(directory, clock = { now }, engineFactory = { engine })
        try {
            assertEquals(VoiceRecorderResult.RecordingStarted, recorder.start())
            assertEquals(
                VoiceRecorderResult.Failed(VoiceRecorderFailure.ALREADY_RECORDING),
                recorder.start(),
            )
            now += 1_275L
            repeat(4) { recorder.sampleAmplitude() }

            val result = recorder.stop()
            assertTrue(result is VoiceRecorderResult.RecordingReady)
            val voice = (result as VoiceRecorderResult.RecordingReady).voice
            assertEquals(1_275L, voice.durationMs)
            assertEquals("audio/ogg", voice.mimeType)
            assertTrue(voice.fileName.endsWith(".ogg"))
            assertTrue(voice.path.startsWith(directory.absolutePath))
            assertEquals(4L, voice.sizeBytes)
            assertEquals(listOf(0, 10, 20, 31), decodeTelegramVoiceWaveform(voice.waveform))
            assertTrue(voice.file.isFile)
            assertTrue(engine.stopped)
            assertTrue(engine.released)
        } finally {
            recorder.close()
            directory.deleteRecursively()
        }
    }

    @Test
    fun cancelAndCloseReleaseResourcesAndDeletePartialRecording() {
        val directory = Files.createTempDirectory("voice-cancel").toFile()
        val engine = FakeEngine(writePartialOnStart = true)
        val recorder = controller(directory, engineFactory = { engine })
        assertEquals(VoiceRecorderResult.RecordingStarted, recorder.start())
        val partial = directory.listFiles().orEmpty().singleOrNull()
        assertNotNull(partial)

        assertTrue(recorder.cancel())
        assertFalse(partial!!.exists())
        assertTrue(engine.released)
        assertFalse(recorder.cancel())

        val secondEngine = FakeEngine(writePartialOnStart = true)
        val closeRecorder = controller(directory, engineFactory = { secondEngine })
        assertEquals(VoiceRecorderResult.RecordingStarted, closeRecorder.start())
        val secondPartial = directory.listFiles().orEmpty().single()
        closeRecorder.close()
        closeRecorder.close()
        assertTrue(secondEngine.released)
        assertFalse(secondPartial.exists())
        directory.deleteRecursively()
    }

    @Test
    fun startOrFinalizeFailureDeletesOutputAndReleasesEngine() {
        val directory = Files.createTempDirectory("voice-failure").toFile()
        val failedStart = FakeEngine(failStart = true, writePartialOnStart = true)
        val startRecorder = controller(directory, engineFactory = { failedStart })
        assertEquals(
            VoiceRecorderResult.Failed(VoiceRecorderFailure.OGG_OPUS_UNAVAILABLE),
            startRecorder.start(),
        )
        assertTrue(failedStart.released)
        assertTrue(directory.listFiles().orEmpty().isEmpty())

        val failedStop = FakeEngine(failStop = true)
        var now = 0L
        val stopRecorder = controller(directory, clock = { now }, engineFactory = { failedStop })
        assertEquals(VoiceRecorderResult.RecordingStarted, stopRecorder.start())
        now = 1_200L
        assertEquals(
            VoiceRecorderResult.Failed(VoiceRecorderFailure.FINALIZATION_FAILED),
            stopRecorder.stop(),
        )
        assertTrue(failedStop.released)
        assertTrue(directory.listFiles().orEmpty().isEmpty())
        startRecorder.close()
        stopRecorder.close()
        directory.deleteRecursively()
    }

    @Test
    fun invalidDurationsAndEmptyOutputAreDiscarded() {
        val directory = Files.createTempDirectory("voice-duration").toFile()
        var now = 0L
        val tooShort = controller(directory, clock = { now }, engineFactory = { FakeEngine() })
        assertEquals(VoiceRecorderResult.RecordingStarted, tooShort.start())
        now = 999L
        assertEquals(
            VoiceRecorderResult.Failed(VoiceRecorderFailure.DURATION_OUT_OF_RANGE),
            tooShort.stop(),
        )
        assertTrue(directory.listFiles().orEmpty().isEmpty())
        tooShort.close()

        now = 0L
        val tooLong = controller(directory, clock = { now }, engineFactory = { FakeEngine() })
        assertEquals(VoiceRecorderResult.RecordingStarted, tooLong.start())
        now = 48L * 60L * 60L * 1_000L + 1L
        assertEquals(
            VoiceRecorderResult.Failed(VoiceRecorderFailure.DURATION_OUT_OF_RANGE),
            tooLong.stop(),
        )
        assertTrue(directory.listFiles().orEmpty().isEmpty())
        tooLong.close()

        now = 0L
        val empty = controller(directory, clock = { now }, engineFactory = { FakeEngine(writeOnStop = false) })
        assertEquals(VoiceRecorderResult.RecordingStarted, empty.start())
        now = 1_000L
        assertEquals(
            VoiceRecorderResult.Failed(VoiceRecorderFailure.FINALIZATION_FAILED),
            empty.stop(),
        )
        assertTrue(directory.listFiles().orEmpty().isEmpty())
        empty.close()
        directory.deleteRecursively()
    }

    private fun controller(
        directory: File,
        apiLevel: Int = 35,
        hasPermission: Boolean = true,
        clock: () -> Long = { 1_000L },
        engineFactory: () -> VoiceRecordingEngine,
    ) = VoiceRecordingController(
        outputDirectory = directory,
        apiLevel = apiLevel,
        hasMicrophonePermission = { hasPermission },
        engineFactory = engineFactory,
        elapsedRealtimeMs = clock,
        fileName = { "test-recording.ogg" },
    )

    private class FakeEngine(
        private val failStart: Boolean = false,
        private val failStop: Boolean = false,
        private val writePartialOnStart: Boolean = false,
        private val writeOnStop: Boolean = true,
        amplitudes: List<Int> = emptyList(),
    ) : VoiceRecordingEngine {
        private lateinit var output: File
        private val amplitudeIterator = amplitudes.iterator()
        var stopped = false
        var released = false

        override fun prepareAndStart(output: File) {
            this.output = output
            if (writePartialOnStart) output.writeBytes(byteArrayOf(1))
            if (failStart) throw IOException("no Ogg/Opus encoder")
        }

        override fun stop() {
            stopped = true
            if (failStop) throw IOException("finalization failed")
            if (writeOnStop) output.writeBytes(byteArrayOf(1, 2, 3, 4))
        }

        override fun release() {
            released = true
        }

        override fun sampleAmplitude(): Int? = amplitudeIterator.takeIf { it.hasNext() }?.next()
    }
}
