package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class AsyncContentStatesTest {
    @Test
    fun resolvesLoadingEmptyErrorAndContentBranches() {
        assertEquals(
            AsyncContentMode.Loading,
            resolveAsyncContentMode(isLoading = true, isEmpty = true, hasError = false),
        )
        assertEquals(
            AsyncContentMode.Empty,
            resolveAsyncContentMode(isLoading = false, isEmpty = true, hasError = false),
        )
        assertEquals(
            AsyncContentMode.Error,
            resolveAsyncContentMode(isLoading = false, isEmpty = true, hasError = true),
        )
        assertEquals(
            AsyncContentMode.Content,
            resolveAsyncContentMode(isLoading = false, isEmpty = false, hasError = false),
        )
    }

    @Test
    fun emptyLoadingStateTakesPrecedenceOverStaleError() {
        assertEquals(
            AsyncContentMode.Loading,
            resolveAsyncContentMode(isLoading = true, isEmpty = true, hasError = true),
        )
    }

    @Test
    fun existingContentStaysVisibleWhileLoadingOrAfterError() {
        assertEquals(
            AsyncContentMode.Content,
            resolveAsyncContentMode(isLoading = true, isEmpty = false, hasError = false),
        )
        assertEquals(
            AsyncContentMode.Content,
            resolveAsyncContentMode(isLoading = false, isEmpty = false, hasError = true),
        )
    }
}
