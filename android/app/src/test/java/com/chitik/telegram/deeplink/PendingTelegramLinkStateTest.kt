package com.chitik.telegram.deeplink

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PendingTelegramLinkStateTest {
    @Test
    fun unrelatedNewIntentDoesNotDiscardPendingLinkReview() {
        val pending = requireNotNull(TelegramDeepLinkRouter.parse("tg://join?invite=abc_123"))

        assertEquals(pending, pendingTelegramLinkAfterNewIntent(pending, incoming = null))
    }

    @Test
    fun newlyReceivedSupportedLinkReplacesPendingRoute() {
        val pending = requireNotNull(TelegramDeepLinkRouter.parse("tg://join?invite=abc_123"))
        val incoming = requireNotNull(TelegramDeepLinkRouter.parse("https://t.me/example_1/42"))

        assertEquals(incoming, pendingTelegramLinkAfterNewIntent(pending, incoming))
    }

    @Test
    fun unrelatedIntentDoesNotCreateANewPendingRoute() {
        assertNull(pendingTelegramLinkAfterNewIntent(current = null, incoming = null))
    }
}
