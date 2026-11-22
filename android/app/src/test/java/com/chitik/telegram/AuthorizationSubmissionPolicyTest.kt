package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AuthorizationSubmissionPolicyTest {
    @Test
    fun allowsOnlyOneSubmissionForTheCurrentAuthorizationStep() {
        val steps = listOf(
            AuthorizationUiStep.WaitingForPhoneNumber,
            AuthorizationUiStep.WaitingForCode,
            AuthorizationUiStep.WaitingForPassword,
        )

        for (current in steps) {
            for (expected in steps) {
                assertEquals(
                    "expected $expected at $current",
                    current == expected,
                    canSubmitAuthorizationInput(current, expected, isLoading = false),
                )
            }
            assertFalse(
                "duplicate callback accepted at $current",
                canSubmitAuthorizationInput(current, current, isLoading = true),
            )
        }
    }

    @Test
    fun rejectsSubmissionsOutsideAnAuthorizationInputStep() {
        for (step in AuthorizationUiStep.entries - setOf(
                AuthorizationUiStep.WaitingForPhoneNumber,
                AuthorizationUiStep.WaitingForCode,
                AuthorizationUiStep.WaitingForPassword,
            )
        ) {
            assertFalse(canSubmitAuthorizationInput(step, AuthorizationUiStep.WaitingForCode, false))
        }
    }

    @Test
    fun floodWaitBlocksEveryAuthorizationStepUntilCountdownReachesZero() {
        val inputSteps = listOf(
            AuthorizationUiStep.WaitingForPhoneNumber,
            AuthorizationUiStep.WaitingForCode,
            AuthorizationUiStep.WaitingForPassword,
        )
        for (step in inputSteps) {
            assertFalse(
                canSubmitAuthorizationInput(
                    currentStep = step,
                    expectedStep = step,
                    isLoading = false,
                    retryAfterSeconds = 1,
                ),
            )
            assertTrue(
                canSubmitAuthorizationInput(
                    currentStep = step,
                    expectedStep = step,
                    isLoading = false,
                    retryAfterSeconds = 0,
                ),
            )
        }
    }
}
