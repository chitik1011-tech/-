package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AuthorizationRecoveryActionTest {
    @Test
    fun codeAndPasswordStepsOfferTheSamePhoneNumberRecoveryAction() {
        val recoverableSteps = listOf(
            AuthorizationUiStep.WaitingForCode,
            AuthorizationUiStep.WaitingForPassword,
        )
        for (step in recoverableSteps) {
            assertEquals(
                AUTHORIZATION_RECOVERY_ACTION_LABEL,
                authorizationRecoveryActionLabel(
                    AuthorizationUiState(authorization = step, isTelegramConfigured = true),
                ),
            )
        }
    }

    @Test
    fun recoveryActionIsHiddenWhenItCannotRestartAuthorization() {
        val stepsWithoutRecovery = AuthorizationUiStep.entries - setOf(
            AuthorizationUiStep.WaitingForCode,
            AuthorizationUiStep.WaitingForPassword,
        )
        for (step in stepsWithoutRecovery) {
            assertNull(
                "unexpected recovery action for $step",
                authorizationRecoveryActionLabel(
                    AuthorizationUiState(authorization = step, isTelegramConfigured = true),
                ),
            )
        }

        assertNull(
            authorizationRecoveryActionLabel(
                AuthorizationUiState(
                    authorization = AuthorizationUiStep.WaitingForCode,
                    isTelegramConfigured = false,
                ),
            ),
        )
    }

    @Test
    fun expiredCodeErrorKeepsRecoveryAvailable() {
        val state = AuthorizationUiState(
            authorization = AuthorizationUiStep.WaitingForCode,
            isTelegramConfigured = true,
            error = "PHONE_CODE_EXPIRED: request a new login code",
        )

        assertEquals(AUTHORIZATION_RECOVERY_ACTION_LABEL, authorizationRecoveryActionLabel(state))
        assertEquals("PHONE_CODE_EXPIRED: request a new login code", state.error)
    }
}
