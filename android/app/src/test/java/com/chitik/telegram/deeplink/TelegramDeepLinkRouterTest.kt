package com.chitik.telegram.deeplink

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TelegramDeepLinkRouterTest {
    @Test
    fun acceptsOnlyViewIntentAction() {
        assertEquals(
            TelegramLinkTarget.Peer("example_1"),
            TelegramDeepLinkRouter.parseIntent(
                TelegramDeepLinkRouter.VIEW_ACTION,
                "https://t.me/example_1",
            )?.target,
        )
        assertNull(TelegramDeepLinkRouter.parseIntent("android.intent.action.SEND", "https://t.me/example_1"))
    }

    @Test
    fun parsesPublicProfileAndResolveLinks() {
        assertEquals(
            TelegramLinkTarget.Peer("example_1", "payload"),
            TelegramDeepLinkRouter.parse("https://t.me/example_1?start=payload")?.target,
        )
        assertEquals(
            TelegramLinkTarget.Peer("example_1", "payload"),
            TelegramDeepLinkRouter.parse("tg://resolve?domain=example_1&start=payload")?.target,
        )
    }

    @Test
    fun parsesPublicAndPrivateMessageLinksWithMetadata() {
        assertEquals(
            TelegramLinkTarget.Message("example_1", 42, threadId = 7, commentId = 9, single = true),
            TelegramDeepLinkRouter.parse("https://www.t.me/s/example_1/42?thread=7&comment=9&single")?.target,
        )
        assertEquals(
            TelegramLinkTarget.PrivateChannelMessage(123456, 42),
            TelegramDeepLinkRouter.parse("https://t.me/c/123456/42")?.target,
        )
        assertEquals(
            TelegramLinkTarget.Message("example_1", 42),
            TelegramDeepLinkRouter.parse("tg://resolve?domain=example_1&post=42")?.target,
        )
    }

    @Test
    fun inviteLinksAreNormalizedButNeverJoinedByParser() {
        for (link in listOf(
            "https://t.me/+AbCdEf_123",
            "https://t.me/joinchat/AbCdEf_123",
            "tg://join?invite=AbCdEf_123",
        )) {
            val parsed = requireNotNull(TelegramDeepLinkRouter.parse(link))
            assertEquals(TelegramLinkTarget.Invite("AbCdEf_123"), parsed.target)
            assertTrue(parsed.requiresConfirmation)
        }
    }

    @Test
    fun messageComposeLinkDecodesUtf8AndDoesNotSend() {
        val parsed = requireNotNull(TelegramDeepLinkRouter.parse("tg://msg?to=example_1&text=Hi%20%F0%9F%98%80"))
        assertEquals(
            TelegramLinkTarget.ComposeMessage("example_1", "Hi 😀", null),
            parsed.target,
        )
        assertFalse(parsed.requiresConfirmation)
    }

    @Test
    fun rejectsMalformedHostsPathsAndIdentifiers() {
        val malformed = listOf(
            "https://t.me.evil.test/example_1",
            "https://user@t.me/example_1",
            "https://t.me/example_1/%2F42",
            "https://t.me/example_1/0",
            "https://t.me/c/123/nope",
            "https://t.me/+",
            "tg://join?invite=",
            "tg://resolve?domain=абвгд",
            "tg://resolve?domain=example_1&domain=other_1",
            "tg://msg?text=" + "x".repeat(4097),
            "file:///etc/passwd",
        )
        malformed.forEach { assertNull("Should reject $it", TelegramDeepLinkRouter.parse(it)) }
    }

    @Test
    fun acceptsBareTelegramHostAndRejectsUntrustedRecipientUrl() {
        assertEquals(
            TelegramLinkTarget.Peer("example_1"),
            TelegramDeepLinkRouter.parse("t.me/example_1")?.target,
        )
        assertNull(TelegramDeepLinkRouter.parse("tg://msg?url=javascript%3Aalert(1)"))
    }
}