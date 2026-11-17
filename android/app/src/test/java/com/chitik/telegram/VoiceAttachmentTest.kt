package com.chitik.telegram

import java.io.File
import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VoiceAttachmentTest {
    @Test
    fun waveformIsBoundedAndHandlesInvalidSamples() {
        assertEquals(emptyList<Int>(), formatVoiceWaveform(emptyList()))
        assertEquals(emptyList<Int>(), formatVoiceWaveform(listOf(1, 2), maxBars = 0))
        val bars = formatVoiceWaveform(listOf(-4, 0, 8, 31, 80), maxBars = 3)
        assertEquals(3, bars.size)
        assertTrue(bars.all { it in 4..24 })
        assertTrue(bars.last() > bars.first())
    }

    @Test
    fun coordinatorKeepsOneActiveFileAndDoesNotResumePausedPlayback() {
        val dir = Files.createTempDirectory("voice-playback-test").toFile()
        val first = File(dir, "first.ogg").apply { writeBytes(byteArrayOf(1)) }
        val second = File(dir, "second.ogg").apply { writeBytes(byteArrayOf(2)) }
        val player = FakeVoicePlayer()
        val coordinator = VoicePlaybackCoordinator(player, FakeScheduler())
        try {
            coordinator.toggle(first)
            assertEquals(first.absolutePath, coordinator.state.filePath)
            assertEquals(1, player.playCalls)

            coordinator.toggle(second)
            assertEquals(second.absolutePath, coordinator.state.filePath)
            assertEquals(2, player.playCalls)
            assertEquals(1, player.stopCalls)

            coordinator.onHostStop()
            coordinator.onHostStart()
            assertEquals("paused playback must not resume", 2, player.playCalls)
        } finally {
            coordinator.release()
            dir.deleteRecursively()
        }
        assertTrue(player.released)
    }

    @Test
    fun lifecyclePauseResumesOnceAcrossMultipleCardObservers() {
        val dir = Files.createTempDirectory("voice-lifecycle-test").toFile()
        val audio = File(dir, "voice.ogg").apply { writeBytes(byteArrayOf(1)) }
        val player = FakeVoicePlayer()
        val coordinator = VoicePlaybackCoordinator(player, FakeScheduler())
        try {
            coordinator.toggle(audio)
            player.currentListener?.onPlayingChanged(true)
            coordinator.onHostStop()
            coordinator.onHostStop()
            coordinator.onHostStart()
            coordinator.onHostStart()
            assertEquals("resumes only once", 2, player.playCalls)
            assertFalse(coordinator.state.isPlaying)
        } finally {
            coordinator.release()
            dir.deleteRecursively()
        }
    }

    private class FakeScheduler : VoicePlaybackScheduler {
        override fun postDelayed(task: Runnable, delayMs: Long) = Unit
        override fun remove(task: Runnable) = Unit
    }

    private class FakeVoicePlayer : VoicePlayerPort {
        var currentListener: VoicePlayerPort.Listener? = null
        var playCalls = 0
        var stopCalls = 0
        var released = false
        override fun play(file: File) { playCalls++ }
        override fun pause() { currentListener?.onPlayingChanged(false) }
        override fun stop() { stopCalls++ }
        override fun seekTo(positionMs: Long) = Unit
        override fun currentPositionMs(): Long = 0L
        override fun durationMs(): Long = 0L
        override fun setListener(listener: VoicePlayerPort.Listener?) { currentListener = listener }
        override fun release() { released = true }
    }
}
