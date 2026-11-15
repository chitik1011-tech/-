package com.chitik.telegram.deeplink

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class TelegramLinkAppRouterTest {
    @Test
    fun everyPeerAndMessageLinkRoutesToTransientReview() {
        val peer = requireNotNull(TelegramDeepLinkRouter.parse("https://t.me/example_1"))
        val message = requireNotNull(TelegramDeepLinkRouter.parse("https://t.me/example_1/22"))

        assertEquals(TelegramLinkDestination.LinkReview, TelegramLinkAppRouter.route(peer).destination)
        assertEquals(peer, TelegramLinkAppRouter.route(peer).link)
        assertEquals(TelegramLinkTarget.Message("example_1", 22), TelegramLinkAppRouter.route(message).link.target)
        assertNull(TelegramLinkAppRouter.confirmInvite(TelegramLinkAppRouter.route(peer)))
    }

    @Test
    fun everyPrivateChannelAndComposeLinkRoutesToTransientReview() {
        val channelMessage = requireNotNull(TelegramDeepLinkRouter.parse("https://t.me/c/123456/22"))
        val compose = requireNotNull(TelegramDeepLinkRouter.parse("tg://msg?to=example_1&text=Hi"))

        assertEquals(TelegramLinkDestination.LinkReview, TelegramLinkAppRouter.route(channelMessage).destination)
        assertEquals(
            TelegramLinkTarget.PrivateChannelMessage(123456, 22),
            TelegramLinkAppRouter.route(channelMessage).link.target,
        )
        assertEquals(TelegramLinkDestination.LinkReview, TelegramLinkAppRouter.route(compose).destination)
        assertEquals(
            TelegramLinkTarget.ComposeMessage("example_1", "Hi", null),
            TelegramLinkAppRouter.route(compose).link.target,
        )
    }

    @Test
    fun joinIntentExistsOnlyAfterExplicitInviteConfirmation() {
        val invite = requireNotNull(TelegramDeepLinkRouter.parse("tg://join?invite=AbCdEf_123"))
        val pendingRoute = TelegramLinkAppRouter.route(invite)

        assertEquals(TelegramLinkDestination.LinkReview, pendingRoute.destination)
        assertEquals(TelegramLinkTarget.Invite("AbCdEf_123"), pendingRoute.link.target)
        assertEquals(true, pendingRoute.link.requiresConfirmation)
        assertEquals(TelegramJoinInviteIntent("AbCdEf_123"), TelegramLinkAppRouter.confirmInvite(pendingRoute))
    }
}