package com.chitik.telegram

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ChatCapabilitiesUiTest {
    @Test
    fun parsesOnlyExplicitKnownPermissions() {
        val parsed = parseChatCapabilities(
            JSONObject(
                """{"42":{
                    "can_send_now":{"availability":"known","value":false},
                    "can_send_messages":{"availability":"known","value":true},
                    "can_send_media":{"availability":"unknown"},
                    "slow_mode_enabled":{"availability":"known","value":true},
                    "slow_mode_remaining_seconds":{"availability":"known","value":8}
                }}""",
            ),
        ).getValue(42L)

        assertEquals(false, parsed.canSendNow)
        assertEquals(true, parsed.canSendMessages)
        assertNull(parsed.canSendMedia)
        assertEquals(true, parsed.slowModeEnabled)
        assertEquals(8L, parsed.slowModeRemainingSeconds)
    }

    @Test
    fun malformedOrMissingSnapshotsRemainUnknown() {
        assertNull(parseChatCapabilities(null)[1L]?.canSendNow)
        assertNull(
            parseChatCapabilities(JSONObject("""{"1":{"can_send_now":{"availability":"unknown"}}}"""))
                .getValue(1L)
                .canSendNow,
        )
        assertNull(
            parseChatCapabilities(JSONObject("""{"1":{"can_send_now":{"availability":"known","value":"yes"}}}"""))
                .getValue(1L)
                .canSendNow,
        )
    }
}
