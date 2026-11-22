package com.chitik.telegram.nativebridge

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectionRecoveryPolicyTest {
    @Test
    fun retriesTerminalFailureOnlyForAuthorizedSessionOnValidatedNetwork() {
        assertTrue(shouldAutomaticallyRecoverConnection(true, true, true, 0))
        assertFalse(shouldAutomaticallyRecoverConnection(false, true, true, 0))
        assertFalse(shouldAutomaticallyRecoverConnection(true, false, true, 0))
        assertFalse(shouldAutomaticallyRecoverConnection(true, true, false, 0))
    }

    @Test
    fun limitsAutomaticReplacementAttemptsUntilNetworkChanges() {
        assertTrue(shouldAutomaticallyRecoverConnection(true, true, true, 1))
        assertFalse(
            shouldAutomaticallyRecoverConnection(
                true,
                true,
                true,
                MAX_AUTOMATIC_CONNECTION_RECOVERIES,
            ),
        )
    }
}
