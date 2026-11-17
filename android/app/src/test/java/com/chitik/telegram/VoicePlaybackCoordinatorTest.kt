package com.chitik.telegram

import java.io.File
import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VoicePlaybackCoordinatorTest {
    @Test
    fun callbacksAndProgressFromReplacedPlayerCannotMutateCurrentPlayback() {
        val directory = Files.createTempDirectory("voice-playback-epoch").toFile()
        val firstFile = File(directory, "first.ogg").apply { writeBytes(byteArrayOf(1)) }
        val secondFile = File(directory, "second.ogg").apply { writeBytes(byteArrayOf(2)) }
        val first = FakeVoicePlayer(positionMs = 900L, durationMs = 1_500L)
        val second = FakeVoicePlayer(positionMs = 10L, durationMs = 2_000L)
        val players = mutableListOf(first, second)
        val scheduler = FakeScheduler()
        val coordinator = VoicePlaybackCoordinator(VoicePlayerFactory { players.removeAt(0) }, scheduler)
        try {
            coordinator.toggle(firstFile)
            val staleListener = first.currentListener!!
            staleListener.onPlayingChanged(true)
            val staleProgress = scheduler.tasks.single()

            coordinator.toggle(secondFile)
            assertTrue(first.released)
            assertEquals(secondFile.absolutePath, coordinator.state.filePath)
            assertFalse(coordinator.state.isPlaying)

            staleListener.onError("late decoder error")
            staleListener.onCompleted()
            staleListener.onPlayingChanged(true)
            staleProgress.run() // Simulate a delayed Handler callback after replacement.

            assertEquals(secondFile.absolutePath, coordinator.state.filePath)
            assertFalse(coordinator.state.isPlaying)
            assertEquals(0L, coordinator.state.positionMs)
            assertNull(coordinator.state.error)
            assertFalse(second.released)
        } finally {
            coordinator.release()
            directory.deleteRecursively()
        }
    }

    @Test
    fun decoderFailureReleasesCurrentPlayerAndAllowsCleanRetry() {
        val directory = Files.createTempDirectory("voice-playback-error").toFile()
        val file = File(directory, "voice.ogg").apply { writeBytes(byteArrayOf(1)) }
        val first = FakeVoicePlayer()
        val retry = FakeVoicePlayer()
        val players = mutableListOf(first, retry)
        val coordinator = VoicePlaybackCoordinator(VoicePlayerFactory { players.removeAt(0) }, FakeScheduler())
        try {
            coordinator.toggle(file)
            val failedListener = first.currentListener!!
            failedListener.onError("decoder failed")

            assertTrue(first.released)
            assertEquals(file.absolutePath, coordinator.state.filePath)
            assertEquals("decoder failed", coordinator.state.error)
            assertFalse(coordinator.state.isPlaying)

            // A delayed completion from the failed epoch must not erase its error.
            failedListener.onCompleted()
            assertEquals("decoder failed", coordinator.state.error)

            coordinator.toggle(file)
            assertEquals(1, retry.playCalls)
            assertFalse(retry.released)
            assertEquals(file.absolutePath, coordinator.state.filePath)
            assertNull(coordinator.state.error)
        } finally {
            coordinator.release()
            directory.deleteRecursively()
        }
    }

    @Test
    fun playerCreationAndPlayFailuresBecomeStateErrorsWithoutLeakingPlayer() {
        val directory = Files.createTempDirectory("voice-playback-failure").toFile()
        val file = File(directory, "voice.ogg").apply { writeBytes(byteArrayOf(1)) }
        try {
            val creationFailure = VoicePlaybackCoordinator(
                VoicePlayerFactory { throw IllegalStateException("creation failed") },
                FakeScheduler(),
            )
            creationFailure.toggle(file)
            assertEquals(file.absolutePath, creationFailure.state.filePath)
            assertTrue(creationFailure.state.error?.contains("инициализировать") == true)
            creationFailure.release()

            val brokenPlayer = FakeVoicePlayer(failPlay = true)
            val playbackFailure = VoicePlaybackCoordinator(
                VoicePlayerFactory { brokenPlayer },
                FakeScheduler(),
            )
            playbackFailure.toggle(file)
            assertTrue(brokenPlayer.released)
            assertEquals(file.absolutePath, playbackFailure.state.filePath)
            assertTrue(playbackFailure.state.error?.contains("воспроизвести") == true)
            playbackFailure.release()
        } finally {
            directory.deleteRecursively()
        }
    }

    private class FakeScheduler : VoicePlaybackScheduler {
        val tasks = mutableListOf<Runnable>()

        override fun postDelayed(task: Runnable, delayMs: Long) {
            tasks += task
        }

        override fun remove(task: Runnable) {
            tasks.remove(task)
        }
    }

    private class FakeVoicePlayer(
        private val positionMs: Long = 0L,
        private val durationMs: Long = 0L,
        private val failPlay: Boolean = false,
    ) : VoicePlayerPort {
        var currentListener: VoicePlayerPort.Listener? = null
        var playCalls = 0
        var released = false

        override fun play(file: File) {
            playCalls++
            if (failPlay) error("prepare/play failure")
        }

        override fun pause() = Unit
        override fun stop() = Unit
        override fun seekTo(positionMs: Long) = Unit
        override fun currentPositionMs(): Long = positionMs
        override fun durationMs(): Long = durationMs
        override fun setListener(listener: VoicePlayerPort.Listener?) {
            currentListener = listener
        }

        override fun release() {
            released = true
        }
    }
}
