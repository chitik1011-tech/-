package com.chitik.telegram.nativebridge

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectionLifecycleRecoveryTest {
    @Test
    fun terminalConnectFailureRequestsRecoveryButRetryableFailureDoesNot() {
        val terminal = NativeUiState().applyConnectionLifecycle(
            """{"payload":{"token":{"generation":2,"epoch":3},"event":{"type":"connect_failed","retryable":false}}}""",
        )
        assertTrue(terminal.connectionRecoveryNeeded)

        val retryable = NativeUiState().applyConnectionLifecycle(
            """{"payload":{"token":{"generation":2,"epoch":3},"event":{"type":"connect_failed","retryable":true}}}""",
        )
        assertFalse(retryable.connectionRecoveryNeeded)
    }

    @Test
    fun transportConnectionClearsRequestWithoutResettingNetworkAttemptBudget() {
        val recovered = NativeUiState(
            connectionRecoveryNeeded = true,
            connectionRecoveryAttempts = 2,
        ).applyConnectionLifecycle(
            """{"payload":{"token":{"generation":2,"epoch":3},"event":{"type":"transport_connected"}}}""",
        )
        assertFalse(recovered.connectionRecoveryNeeded)
        assertTrue(recovered.connectionRecoveryAttempts == 2)
    }
}
