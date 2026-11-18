package com.chitik.telegram.notifications

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationAccountScopePolicyTest {
    @Test
    fun keepsNoticesOnlyForTheResolvedActiveAccount() {
        assertTrue(NotificationAccountScopePolicy.belongsToActiveAccount(123L, 123L))
        assertFalse(NotificationAccountScopePolicy.belongsToActiveAccount(122L, 123L))
        assertFalse(NotificationAccountScopePolicy.belongsToActiveAccount(null, 123L))
        assertFalse(NotificationAccountScopePolicy.belongsToActiveAccount(123L, null))
        assertFalse(NotificationAccountScopePolicy.belongsToActiveAccount(123L, 0L))
    }

    @Test
    fun sameAccountHandleRecreationRetainsExistingNotices() {
        // Notification identity is account plus chat, not native handle generation. Recreating
        // a handle for the same Telegram user therefore leaves its already posted notice valid.
        assertTrue(NotificationAccountScopePolicy.belongsToActiveAccount(9001L, 9001L))
        assertFalse(NotificationAccountScopePolicy.belongsToActiveAccount(9001L, 9002L))
    }
}
