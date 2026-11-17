package com.chitik.telegram.notifications

import org.junit.Assert.assertEquals
import org.junit.Test

class NotificationRouteGateTest {
    @Test
    fun waitsForAuthorizationAndEmptyInitialSnapshotBeforeConsumingTap() {
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
    }

    @Test
    fun targetFoundOnLaterPageOpensEvenWhileTheListRemainsPartial() {
        assertEquals(
            NotificationRouteResolution.Wait,
            resolveNotificationRoute(
                chatId = 5L,
                authorized = true,
                chatsLoaded = true,
                availableChatIds = listOf(1L, 2L),
                chatListExhausted = false,
            ),
        )
        assertEquals(
            NotificationRouteResolution.OpenChat(5L),
            resolveNotificationRoute(
                chatId = 5L,
                authorized = true,
                chatsLoaded = true,
                availableChatIds = listOf(3L, 5L),
                chatListExhausted = false,
            ),
        )
    }

    @Test
    fun missingTargetStaysPendingUntilAllPagesAreConfirmedExhausted() {
        // Existing call sites without the pagination signal must fail closed and keep the tap.
        assertEquals(
            NotificationRouteResolution.Wait,
            resolveNotificationRoute(
                chatId = 5L,
                authorized = true,
                chatsLoaded = true,
                availableChatIds = listOf(-7L),
            ),
        )
        assertEquals(
            NotificationRouteResolution.Wait,
            resolveNotificationRoute(
                chatId = 5L,
                authorized = true,
                chatsLoaded = true,
                availableChatIds = listOf(-7L),
                chatListExhausted = false,
            ),
        )
        assertEquals(
            NotificationRouteResolution.Discard,
            resolveNotificationRoute(
                chatId = 5L,
                authorized = true,
                chatsLoaded = true,
                availableChatIds = listOf(-7L),
                chatListExhausted = true,
            ),
        )
    }

    @Test
    fun staleOrMalformedRouteIsDiscardedWithoutWaitingForPagination() {
        assertEquals(
            NotificationRouteResolution.Discard,
            resolveNotificationRoute(
                chatId = 0L,
                authorized = false,
                chatsLoaded = false,
                availableChatIds = emptyList(),
            ),
        )
        assertEquals(
            NotificationRouteResolution.Discard,
            resolveNotificationRoute(
                chatId = null,
                authorized = true,
                chatsLoaded = true,
                availableChatIds = emptyList(),
            ),
        )
    }
}
