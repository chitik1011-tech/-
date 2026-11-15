package com.chitik.telegram.nativebridge

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NativeSessionLifecycleTest {
    @Test
    fun closeRejectsLateHandleCreationAndCallbacks() {
        val lifecycle = NativeSessionLifecycle()
        val token = lifecycle.beginHandleCreation()
        assertNotNull(token)

        assertNull(lifecycle.close())
        assertFalse(lifecycle.installHandle(token!!, 41L))
        lifecycle.failHandleCreation(token)
        assertFalse(lifecycle.onNativeEvent("error"))
        assertEquals(
            LifecycleDispatchResult.Closed,
            lifecycle.dispatchCommand { error("closed sessions must not dispatch") },
        )
    }

    @Test
    fun closeTransfersHandleExactlyOnceAndBlocksCommands() {
        val lifecycle = NativeSessionLifecycle()
        val token = lifecycle.beginHandleCreation()!!
        assertTrue(lifecycle.installHandle(token, 7L))

        assertEquals(7L, lifecycle.close())
        assertNull(lifecycle.close())
        assertEquals(
            LifecycleDispatchResult.Closed,
            lifecycle.dispatchCommand { error("closed sessions must not dispatch") },
        )
    }

    @Test
    fun startIsCoalescedUntilFailureOrAuthorizationEvent() {
        val lifecycle = readyLifecycle()
        var sends = 0

        assertEquals(
            LifecycleDispatchResult.Accepted,
            lifecycle.dispatchStart { sends += 1; it == 7L },
        )
        assertEquals(
            LifecycleDispatchResult.Duplicate,
            lifecycle.dispatchStart { sends += 1; true },
        )
        assertTrue(lifecycle.onNativeEvent("error"))
        assertEquals(
            LifecycleDispatchResult.Accepted,
            lifecycle.dispatchStart { sends += 1; true },
        )
        assertTrue(lifecycle.onNativeEvent("authorization_changed"))
        assertEquals(
            LifecycleDispatchResult.Duplicate,
            lifecycle.dispatchStart { sends += 1; true },
        )
        assertEquals(2, sends)
    }

    @Test
    fun chatLoadsAreCoalescedUntilResult() {
        val lifecycle = readyLifecycle()
        var sends = 0

        assertEquals(
            LifecycleDispatchResult.Accepted,
            lifecycle.dispatchLoadChats { sends += 1; true },
        )
        assertEquals(
            LifecycleDispatchResult.Duplicate,
            lifecycle.dispatchLoadChats { sends += 1; true },
        )
        assertTrue(lifecycle.onNativeEvent("chats_loaded"))
        assertEquals(
            LifecycleDispatchResult.Accepted,
            lifecycle.dispatchLoadChats { sends += 1; true },
        )
        assertEquals(2, sends)
    }

    @Test
    fun closeWaitsForAnAlreadyDispatchedCommand() {
        val lifecycle = readyLifecycle()
        val commandEntered = CountDownLatch(1)
        val releaseCommand = CountDownLatch(1)
        val closeFinished = CountDownLatch(1)
        var closedHandle: Long? = null

        val commandThread = thread {
            assertEquals(
                LifecycleDispatchResult.Accepted,
                lifecycle.dispatchCommand {
                    commandEntered.countDown()
                    assertTrue(releaseCommand.await(2, TimeUnit.SECONDS))
                    true
                },
            )
        }
        assertTrue(commandEntered.await(2, TimeUnit.SECONDS))

        val closeThread = thread {
            closedHandle = lifecycle.close()
            closeFinished.countDown()
        }
        assertFalse(closeFinished.await(50, TimeUnit.MILLISECONDS))

        releaseCommand.countDown()
        commandThread.join(2_000)
        closeThread.join(2_000)

        assertEquals(7L, closedHandle)
        assertEquals(
            LifecycleDispatchResult.Closed,
            lifecycle.dispatchCommand { error("closed sessions must not dispatch") },
        )
    }

    private fun readyLifecycle(): NativeSessionLifecycle =
        NativeSessionLifecycle().also { lifecycle ->
            val token = lifecycle.beginHandleCreation()!!
            assertTrue(lifecycle.installHandle(token, 7L))
        }
}
