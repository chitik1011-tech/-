package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Test

class NotificationRouteGateTest {
    @Test
    fun waitsForAuthorizationAndInitialSnapshotThenOpensTargetFoundOnAPage() {
        assertEquals(
            NotificationRouteResolution.Wait,
            resolveNotificationRoute(
                route = TelegramNotificationRoute(5L, 99L),
                authorized = false,
                accountIdentityResolved = false,
                activeAccountUserId = null,
                chatsLoaded = false,
                availableChatIds = emptyList(),
            ),
        )
        assertEquals(
            NotificationRouteResolution.Wait,
            resolveNotificationRoute(
                route = TelegramNotificationRoute(5L, 99L),
                authorized = true,
                accountIdentityResolved = false,
                activeAccountUserId = null,
                chatsLoaded = false,
                availableChatIds = emptyList(),
            ),
        )
        assertEquals(
            NotificationRouteResolution.OpenChat(5L),
            resolveNotificationRoute(
                route = TelegramNotificationRoute(5L, 99L),
                authorized = true,
                accountIdentityResolved = true,
                activeAccountUserId = 99L,
                chatsLoaded = true,
                availableChatIds = listOf(5L, -7L),
                chatListExhausted = false,
            ),
        )
    }

    @Test
    fun missingTargetWaitsUntilPaginationConfirmsExhaustion() {
        assertEquals(
            NotificationRouteResolution.Wait,
            resolveNotificationRoute(
                route = TelegramNotificationRoute(5L, 99L),
                authorized = true,
                accountIdentityResolved = true,
                activeAccountUserId = 99L,
                chatsLoaded = true,
                availableChatIds = listOf(-7L),
                chatListExhausted = false,
            ),
        )
        assertEquals(
            NotificationRouteResolution.Discard,
            resolveNotificationRoute(
                route = TelegramNotificationRoute(5L, 99L),
                authorized = true,
                accountIdentityResolved = true,
                activeAccountUserId = 99L,
                chatsLoaded = true,
                availableChatIds = listOf(-7L),
                chatListExhausted = true,
            ),
        )
        assertEquals(
            NotificationRouteResolution.Discard,
            resolveNotificationRoute(
                route = null,
                authorized = true,
                accountIdentityResolved = true,
                activeAccountUserId = 99L,
                chatsLoaded = true,
                availableChatIds = listOf(5L),
            ),
        )
    }

    @Test
    fun rejectsNotificationTapFromAReplacementAccountEvenWhenChatIdMatches() {
        assertEquals(
            NotificationRouteResolution.Discard,
            resolveNotificationRoute(
                route = TelegramNotificationRoute(5L, 99L),
                authorized = true,
                accountIdentityResolved = true,
                activeAccountUserId = 100L,
                chatsLoaded = true,
                availableChatIds = listOf(5L),
            ),
        )
    }

    @Test
    fun rejectsRouteWhenActiveAccountIdentityIsUnavailable() {
        assertEquals(
            NotificationRouteResolution.Discard,
            resolveNotificationRoute(
                route = TelegramNotificationRoute(5L, 99L),
                authorized = true,
                accountIdentityResolved = true,
                activeAccountUserId = null,
                chatsLoaded = true,
                availableChatIds = listOf(5L),
            ),
        )
    }
}
