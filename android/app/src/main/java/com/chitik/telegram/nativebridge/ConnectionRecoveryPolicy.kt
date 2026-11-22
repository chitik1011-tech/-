package com.chitik.telegram.nativebridge

internal const val MAX_AUTOMATIC_CONNECTION_RECOVERIES = 2

internal fun shouldAutomaticallyRecoverConnection(
    authorized: Boolean,
    terminalFailure: Boolean,
    networkValidated: Boolean,
    attempts: Int,
): Boolean = authorized && terminalFailure && networkValidated &&
    attempts in 0 until MAX_AUTOMATIC_CONNECTION_RECOVERIES
