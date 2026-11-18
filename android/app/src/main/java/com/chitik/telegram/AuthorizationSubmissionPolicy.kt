package com.chitik.telegram

/** Rejects stale or duplicate form callbacks before they reach the native auth queue. */
internal fun canSubmitAuthorizationInput(
    currentStep: AuthorizationUiStep,
    expectedStep: AuthorizationUiStep,
    isLoading: Boolean,
): Boolean = !isLoading && currentStep == expectedStep
