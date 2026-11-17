package com.chitik.telegram.nativebridge

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NativeSessionLifecycleTest {
    @Test
    fun replacementRejectsLateCallbacksFromDetachedGeneration() {
        val lifecycle = NativeSessionLifecycle()
        val first = lifecycle.beginReplacement()!!
        assertTrue(lifecycle.installHandle(first.generation, 7L))
        assertTrue(lifecycle.onNativeEvent(first.generation, "state"))

        val second = lifecycle.beginReplacement()!!
        assertEquals(7L, second.detachedHandle)
        assertFalse(lifecycle.onNativeEvent(first.generation, "message_added"))
        assertTrue(lifecycle.onNativeEvent(second.generation, "state"))
        assertTrue(lifecycle.installHandle(second.generation, 8L))
        assertFalse(lifecycle.onNativeEvent(first.generation, "error"))
        assertFalse(
            lifecycle.onNativeEvent(first.generation, "authorization_code_delivery_changed"),
        )
        assertTrue(lifecycle.onNativeEvent(second.generation, "authorization_changed"))
        assertEquals(8L, lifecycle.currentHandleOrZero())
    }

    @Test
    fun failedCreationCanBeRetriedWithoutReopeningOldHandle() {
        val lifecycle = NativeSessionLifecycle()
        val first = lifecycle.beginReplacement()!!
        lifecycle.failCreation(first.generation)
        assertEquals(0L, lifecycle.currentHandleOrZero())

        val retry = lifecycle.beginReplacement()
        assertNotNull(retry)
        assertTrue(lifecycle.installHandle(retry!!.generation, 9L))
        assertEquals(9L, lifecycle.currentHandleOrZero())
    }

    @Test
    fun closeRejectsLateCreationAndCallbacks() {
        val lifecycle = NativeSessionLifecycle()
        val creation = lifecycle.beginReplacement()!!
        assertNull(lifecycle.close())
        assertFalse(lifecycle.installHandle(creation.generation, 41L))
        lifecycle.failCreation(creation.generation)
        assertFalse(lifecycle.onNativeEvent(creation.generation, "error"))
        assertEquals(
            LifecycleDispatchResult.Closed,
            lifecycle.dispatchCommand { error("closed sessions must not dispatch") },
        )
    }

    @Test
    fun startAndChatLoadsAreCoalescedPerInstalledGeneration() {
        val lifecycle = NativeSessionLifecycle()
        val creation = lifecycle.beginReplacement()!!
        assertTrue(lifecycle.installHandle(creation.generation, 7L))
        var starts = 0
        var loads = 0

        assertEquals(
            LifecycleDispatchResult.Accepted,
            lifecycle.dispatchStart { starts += 1; true },
        )
        assertEquals(
            LifecycleDispatchResult.Duplicate,
            lifecycle.dispatchStart { starts += 1; true },
        )
        assertTrue(lifecycle.onNativeEvent(creation.generation, "authorization_changed"))
        assertEquals(
            LifecycleDispatchResult.Duplicate,
            lifecycle.dispatchStart { starts += 1; true },
        )

        assertEquals(
            LifecycleDispatchResult.Accepted,
            lifecycle.dispatchLoadChats { loads += 1; true },
        )
        assertEquals(
            LifecycleDispatchResult.Duplicate,
            lifecycle.dispatchLoadChats { loads += 1; true },
        )
        assertTrue(lifecycle.onNativeEvent(creation.generation, "chats_loaded"))
        assertEquals(
            LifecycleDispatchResult.Accepted,
            lifecycle.dispatchLoadChats { loads += 1; true },
        )

        assertEquals(1, starts)
        assertEquals(2, loads)
    }

    @Test
    fun replacementWaitsForAlreadyDispatchedCommand() {
        val lifecycle = NativeSessionLifecycle()
        val creation = lifecycle.beginReplacement()!!
        assertTrue(lifecycle.installHandle(creation.generation, 7L))

        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val replacementFinished = CountDownLatch(1)
        val failure = AtomicReference<Throwable?>(null)
        var replacement: NativeHandleCreation? = null

        val commandThread = thread {
            try {
                assertEquals(
                    LifecycleDispatchResult.Accepted,
                    lifecycle.dispatchCommand {
                        entered.countDown()
                        assertTrue(release.await(2, TimeUnit.SECONDS))
                        true
                    },
                )
            } catch (error: Throwable) {
                failure.set(error)
            }
        }
        assertTrue(entered.await(2, TimeUnit.SECONDS))

        val replacementThread = thread {
            replacement = lifecycle.beginReplacement()
            replacementFinished.countDown()
        }
        assertFalse(replacementFinished.await(50, TimeUnit.MILLISECONDS))

        release.countDown()
        commandThread.join(2_000)
        replacementThread.join(2_000)

        assertNull(failure.get())
        assertEquals(7L, replacement?.detachedHandle)
        assertEquals(0L, lifecycle.currentHandleOrZero())
    }
}
