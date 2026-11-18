package com.chitik.telegram

/** Rejects stale or duplicate form callbacks before they reach the native auth queue. */
internal fun canSubmitAuthorizationInput(
    currentStep: AuthorizationUiStep,
    expectedStep: AuthorizationUiStep,
    isLoading: Boolean,
    retryAfterSeconds: Int = 0,
): Boolean = authorizationInputEnabled(isLoading, retryAfterSeconds) && currentStep == expectedStep

internal fun authorizationInputEnabled(
    isLoading: Boolean,
    retryAfterSeconds: Int,
): Boolean = !isLoading && retryAfterSeconds <= 0
