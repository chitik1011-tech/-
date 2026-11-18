package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PeerProfilePresentationTest {
    @Test
    fun usesTelegramFieldsForCompactIdentityAndCapabilityDetails() {
        val presentation = profile(
            kind = "channel",
            firstName = "Nina",
            lastName = "West",
            username = "@nina_news",
            bio = "  News and updates  ",
            memberCount = 1_240,
            isVerified = true,
            isPremium = true,
            canSendMessages = false,
            canInviteUsers = true,
        ).toPeerProfilePresentation()

        assertEquals("Nina West", presentation.title)
        assertEquals("Channel", presentation.kindLabel)
        assertEquals("nina_news", presentation.username)
        assertEquals("News and updates", presentation.bio)
        assertEquals(1_240, presentation.memberCount)
        assertEquals(listOf("Verified", "Premium"), presentation.badges)
        assertEquals(
            listOf(
                PeerProfilePermission("Can send messages", "Unavailable"),
                PeerProfilePermission("Can add members", "Allowed"),
            ),
            presentation.permissions,
        )
        assertFalse(presentation.canMessage)
    }

    @Test
    fun fallsBackToRealNameFieldsAndOmitsUnknownOptionalDetails() {
        val presentation = profile(
            firstName = "  Alex ",
            lastName = "",
            presence = "online",
        ).toPeerProfilePresentation()

        assertEquals("Alex", presentation.title)
        assertEquals("Private chat", presentation.kindLabel)
        assertEquals("Online", presentation.presence)
        assertEquals(null, presentation.username)
        assertEquals(null, presentation.bio)
        assertEquals(null, presentation.memberCount)
        assertTrue(presentation.badges.isEmpty())
        assertTrue(presentation.permissions.isEmpty())
        assertTrue(presentation.canMessage)
    }

    @Test
    fun usesUsernameAsLastNameFallbackAndAvoidsDuplicateBotBadge() {
        val presentation = profile(
            kind = "bot",
            username = "weather_bot",
            isBot = true,
        ).toPeerProfilePresentation()

        assertEquals("weather_bot", presentation.title)
        assertEquals("Bot", presentation.kindLabel)
        assertEquals(listOf("Bot"), presentation.badges)
    }

    @Test
    fun publicUsernameActionsNormalizeUsernameAndBuildSafeTelegramUri() {
        assertEquals("nina_news", normalizePublicTelegramUsername("  @nina_news  "))
        assertEquals(
            PeerProfileUsernameActions("@nina_news", "https://t.me/nina_news"),
            peerProfileUsernameActions("@nina_news"),
        )
        assertEquals(null, peerProfileUsernameActions("@nina"))
        assertEquals(null, peerProfileUsernameActions("@nina-news"))
        assertEquals(null, peerProfileUsernameActions("@@nina_news"))
        assertEquals(null, peerProfileUsernameActions("a".repeat(33)))
    }

    @Test
    fun exposesUsernameActionsOnlyForValidPublicUsername() {
        val publicProfile = profile(username = "@nina_news").toPeerProfilePresentation()
        assertEquals("nina_news", publicProfile.username)
        assertEquals("@nina_news", publicProfile.usernameActions?.copyText)
        assertEquals("https://t.me/nina_news", publicProfile.usernameActions?.uri)

        val invalidProfile = profile(username = "private name").toPeerProfilePresentation()
        assertEquals(null, invalidProfile.username)
        assertEquals(null, invalidProfile.usernameActions)
    }

    @Test
    fun doesNotOfferMessageActionForChannelWithoutSendPermission() {
        assertFalse(profile(kind = "channel").toPeerProfilePresentation().canMessage)
        assertTrue(profile(kind = "channel", canSendMessages = true).toPeerProfilePresentation().canMessage)
    }

    private fun profile(
        kind: String = "user",
        displayName: String? = null,
        firstName: String? = null,
        lastName: String? = null,
        username: String? = null,
        bio: String? = null,
        memberCount: Int? = null,
        presence: String? = null,
        isBot: Boolean? = null,
        isVerified: Boolean? = null,
        isPremium: Boolean? = null,
        canSendMessages: Boolean? = null,
        canInviteUsers: Boolean? = null,
        canChangeInfo: Boolean? = null,
        canPostMessages: Boolean? = null,
    ) = PeerProfileUi(
        peerId = 1,
        kind = kind,
        displayName = displayName,
        firstName = firstName,
        lastName = lastName,
        username = username,
        bio = bio,
        memberCount = memberCount,
        members = null,
        membersTruncated = false,
        presence = presence,
        isBot = isBot,
        isVerified = isVerified,
        isPremium = isPremium,
        canSendMessages = canSendMessages,
        canInviteUsers = canInviteUsers,
        canChangeInfo = canChangeInfo,
        canPostMessages = canPostMessages,
        avatarFileId = null,
        avatarLocalPath = null,
    )
}
