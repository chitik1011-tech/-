package com.chitik.telegram.nativebridge

import android.content.Context
import android.content.SharedPreferences

/** Persists only a retry deadline; authorization inputs and credentials are never stored here. */
internal object AuthorizationFloodWaitStore {
    private const val PREFERENCES_NAME = "telegram_auth_retry"
    private const val DEADLINE_KEY = "flood_wait_deadline_epoch_ms"

    fun save(context: Context, seconds: Int, nowMillis: Long = System.currentTimeMillis()) {
        val deadline = authorizationFloodWaitDeadline(nowMillis, seconds)
        preferences(context).edit().putLong(DEADLINE_KEY, deadline).commit()
    }

    fun remainingSeconds(context: Context, nowMillis: Long = System.currentTimeMillis()): Int {
        val deadline = preferences(context).getLong(DEADLINE_KEY, 0L)
        val remaining = authorizationFloodWaitRemainingSeconds(deadline, nowMillis)
        if (remaining == 0) clear(context)
        return remaining
    }

    fun clear(context: Context) {
        preferences(context).edit().remove(DEADLINE_KEY).commit()
    }

    private fun preferences(context: Context): SharedPreferences =
        context.applicationContext.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)
}

internal fun authorizationFloodWaitDeadline(nowMillis: Long, seconds: Int): Long {
    if (seconds <= 0) return nowMillis
    val durationMillis = seconds.toLong() * 1_000L
    return if (Long.MAX_VALUE - nowMillis < durationMillis) Long.MAX_VALUE else nowMillis + durationMillis
}

/** Round up so a sub-second remainder never enables a request before Telegram's deadline. */
internal fun authorizationFloodWaitRemainingSeconds(deadlineMillis: Long, nowMillis: Long): Int {
    if (deadlineMillis <= nowMillis) return 0
    val remainingMillis = deadlineMillis - nowMillis
    val roundedSeconds = remainingMillis / 1_000L + if (remainingMillis % 1_000L == 0L) 0 else 1
    return roundedSeconds.coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
}