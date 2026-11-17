package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Test

class NotificationRouteGateTest {
    @Test
    fun waitsForAuthorizationAndInitialSnapshotThenOpensTargetFoundOnAPage() {
        assertEquals(
            NotificationRouteResolution.Wait,
            resolveNotificationRoute(
                5L,
                authorized = false,
                chatsLoaded = false,
                availableChatIds = emptyList(),
            ),
        )
        assertEquals(
            NotificationRouteResolution.Wait,
            resolveNotificationRoute(
                5L,
                authorized = true,
                chatsLoaded = false,
                availableChatIds = emptyList(),
            ),
        )
        assertEquals(
            NotificationRouteResolution.OpenChat(5L),
            resolveNotificationRoute(
                5L,
                authorized = true,
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
                5L,
                authorized = true,
                chatsLoaded = true,
                availableChatIds = listOf(-7L),
                chatListExhausted = false,
            ),
        )
        assertEquals(
            NotificationRouteResolution.Discard,
            resolveNotificationRoute(
                5L,
                authorized = true,
                chatsLoaded = true,
                availableChatIds = listOf(-7L),
                chatListExhausted = true,
            ),
        )
        assertEquals(
            NotificationRouteResolution.Discard,
            resolveNotificationRoute(
                0L,
                authorized = true,
                chatsLoaded = true,
                availableChatIds = listOf(5L),
            ),
        )
    }
}
