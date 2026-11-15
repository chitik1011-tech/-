package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AuthorizationInputRetentionTest {
    @Test
    fun configurationInputsRemainOnlyUntilRuntimeConfigIsAccepted() {
        val needsConfiguration = authorizationInputsToClear(
            AuthorizationUiState(
                authorization = AuthorizationUiStep.Closed,
                isTelegramConfigured = false,
            ),
        )
        assertFalse(AuthorizationInputField.ApiId in needsConfiguration)
        assertFalse(AuthorizationInputField.ApiHash in needsConfiguration)

        val configured = authorizationInputsToClear(
            AuthorizationUiState(
                authorization = AuthorizationUiStep.WaitingForPhoneNumber,
                isTelegramConfigured = true,
            ),
        )
        assertTrue(AuthorizationInputField.ApiId in configured)
        assertTrue(AuthorizationInputField.ApiHash in configured)
    }

    @Test
    fun eachRustAuthorizationStepRetainsOnlyItsCurrentInput() {
        val phoneStep = authorizationInputsToClear(
            AuthorizationUiState(
                authorization = AuthorizationUiStep.WaitingForPhoneNumber,
                isTelegramConfigured = true,
            ),
        )
        assertFalse(AuthorizationInputField.PhoneNumber in phoneStep)
        assertTrue(AuthorizationInputField.LoginCode in phoneStep)
        assertTrue(AuthorizationInputField.TwoFactorPassword in phoneStep)

        val codeStep = authorizationInputsToClear(
            AuthorizationUiState(
                authorization = AuthorizationUiStep.WaitingForCode,
                isTelegramConfigured = true,
            ),
        )
        assertTrue(AuthorizationInputField.PhoneNumber in codeStep)
        assertFalse(AuthorizationInputField.LoginCode in codeStep)
        assertTrue(AuthorizationInputField.TwoFactorPassword in codeStep)

        val passwordStep = authorizationInputsToClear(
            AuthorizationUiState(
                authorization = AuthorizationUiStep.WaitingForPassword,
                isTelegramConfigured = true,
            ),
        )
        assertTrue(AuthorizationInputField.PhoneNumber in passwordStep)
        assertTrue(AuthorizationInputField.LoginCode in passwordStep)
        assertFalse(AuthorizationInputField.TwoFactorPassword in passwordStep)
    }

    @Test
    fun terminalAuthorizationStateClearsAllTemporaryInputs() {
        val cleared = authorizationInputsToClear(
            AuthorizationUiState(
                authorization = AuthorizationUiStep.Ready,
                isTelegramConfigured = true,
            ),
        )

        assertEquals(AuthorizationInputField.entries.toSet(), cleared)
    }
}
