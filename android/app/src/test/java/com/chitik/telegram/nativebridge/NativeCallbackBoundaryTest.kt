package com.chitik.telegram.nativebridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NativeCallbackBoundaryTest {
    @Test
    fun successfulCallbackIsDeliveredWithoutFailure() {
        var dispatched = false
        var failure: Exception? = null

        dispatchNativeCallbackSafely(
            dispatch = { dispatched = true },
            onFailure = { failure = it },
        )

        assertTrue(dispatched)
        assertEquals(null, failure)
    }

    @Test
    fun callbackExceptionIsReportedAndDoesNotEscape() {
        val expected = IllegalStateException("decode failed")
        var failure: Exception? = null

        dispatchNativeCallbackSafely(
            dispatch = { throw expected },
            onFailure = { failure = it },
        )

        assertEquals(expected, failure)
    }

    @Test
    fun reportingFailureDoesNotEscapeBackThroughJni() {
        var returned = false

        dispatchNativeCallbackSafely(
            dispatch = { throw IllegalStateException("decode failed") },
            onFailure = { throw IllegalStateException("state closed") },
        )
        returned = true

        assertTrue(returned)
    }

    @Test
    fun fatalErrorsAreNotConvertedToOrdinaryCallbackFailures() {
        var failureReported = false
        var escaped = false

        try {
            dispatchNativeCallbackSafely(
                dispatch = { throw AssertionError("fatal") },
                onFailure = { failureReported = true },
            )
        } catch (_: AssertionError) {
            escaped = true
        }

        assertTrue(escaped)
        assertFalse(failureReported)
    }

    @Test
    fun fatalErrorFromFailureReporterIsNotSuppressed() {
        var escaped = false

        try {
            dispatchNativeCallbackSafely(
                dispatch = { throw IllegalStateException("decode failed") },
                onFailure = { throw AssertionError("fatal reporter failure") },
            )
        } catch (_: AssertionError) {
            escaped = true
        }

        assertTrue(escaped)
    }
}
