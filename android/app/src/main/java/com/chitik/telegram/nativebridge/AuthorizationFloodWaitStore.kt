package com.chitik.telegram.nativebridge

import android.content.Context
import android.content.SharedPreferences
import android.os.SystemClock
import android.provider.Settings

/** Persists only a retry deadline; authorization inputs and credentials are never stored here. */
internal object AuthorizationFloodWaitStore {
    private const val PREFERENCES_NAME = "telegram_auth_retry"
    private const val DEADLINE_ELAPSED_KEY = "flood_wait_deadline_elapsed_ms"
    private const val BOOT_COUNT_KEY = "flood_wait_boot_count"
    private const val DURATION_SECONDS_KEY = "flood_wait_duration_seconds"
    private const val LEGACY_DEADLINE_KEY = "flood_wait_deadline_epoch_ms"

    fun save(context: Context, seconds: Int, nowMillis: Long = System.currentTimeMillis()) {
        val prefs = preferences(context)
        val bootCount = currentBootCount(context)
        val duration = seconds.coerceAtLeast(0)
        val editor = prefs.edit()
        if (bootCount >= 0) {
            editor.putLong(
                DEADLINE_ELAPSED_KEY,
                authorizationFloodWaitDeadline(SystemClock.elapsedRealtime(), duration),
            )
                .putInt(BOOT_COUNT_KEY, bootCount)
                .putInt(DURATION_SECONDS_KEY, duration)
                .remove(LEGACY_DEADLINE_KEY)
        } else {
            // Fallback for devices that cannot expose BOOT_COUNT. Android API 26+ normally
            // provides it; keep the older wall-clock format readable on unusual OEM builds.
            editor.putLong(LEGACY_DEADLINE_KEY, authorizationFloodWaitDeadline(nowMillis, duration))
                .remove(DEADLINE_ELAPSED_KEY)
                .remove(BOOT_COUNT_KEY)
                .remove(DURATION_SECONDS_KEY)
        }
        editor.commit()
    }

    fun remainingSeconds(context: Context, nowMillis: Long = System.currentTimeMillis()): Int {
        val prefs = preferences(context)
        val storedBootCount = prefs.getInt(BOOT_COUNT_KEY, -1)
        val currentBootCount = currentBootCount(context)
        if (storedBootCount >= 0 && currentBootCount >= 0) {
            val nowElapsed = SystemClock.elapsedRealtime()
            val storedDeadline = prefs.getLong(DEADLINE_ELAPSED_KEY, 0L)
            val duration = prefs.getInt(DURATION_SECONDS_KEY, 0).coerceAtLeast(0)
            val rearmed = storedBootCount != currentBootCount
            val deadline = authorizationFloodWaitDeadlineForBoot(
                storedDeadlineElapsedMillis = storedDeadline,
                storedBootCount = storedBootCount,
                currentBootCount = currentBootCount,
                durationSeconds = duration,
                nowElapsedMillis = nowElapsed,
            )
            if (rearmed) {
                prefs.edit()
                    .putLong(DEADLINE_ELAPSED_KEY, deadline)
                    .putInt(BOOT_COUNT_KEY, currentBootCount)
                    .commit()
            }
            val remaining = authorizationFloodWaitRemainingSeconds(deadline, nowElapsed)
            if (remaining == 0) clear(context)
            return remaining
        }

        // Migrate an in-progress wait saved by older app versions.
        val legacyDeadline = prefs.getLong(LEGACY_DEADLINE_KEY, 0L)
        val remaining = authorizationFloodWaitRemainingSeconds(legacyDeadline, nowMillis)
        if (remaining > 0 && currentBootCount >= 0) {
            val elapsedDeadline = authorizationFloodWaitDeadline(
                SystemClock.elapsedRealtime(),
                remaining,
            )
            prefs.edit()
                .putLong(DEADLINE_ELAPSED_KEY, elapsedDeadline)
                .putInt(BOOT_COUNT_KEY, currentBootCount)
                .putInt(DURATION_SECONDS_KEY, remaining)
                .remove(LEGACY_DEADLINE_KEY)
                .commit()
        }
        if (remaining == 0) clear(context)
        return remaining
    }

    fun clear(context: Context) {
        preferences(context).edit()
            .remove(DEADLINE_ELAPSED_KEY)
            .remove(BOOT_COUNT_KEY)
            .remove(DURATION_SECONDS_KEY)
            .remove(LEGACY_DEADLINE_KEY)
            .commit()
    }

    private fun preferences(context: Context): SharedPreferences =
        context.applicationContext.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)

    private fun currentBootCount(context: Context): Int = try {
        Settings.Global.getInt(context.contentResolver, Settings.Global.BOOT_COUNT, -1)
    } catch (_: Exception) {
        -1
    }
}

/** A reboot restarts the full Telegram wait because elapsed realtime resets at boot. */
internal fun authorizationFloodWaitDeadlineForBoot(
    storedDeadlineElapsedMillis: Long,
    storedBootCount: Int,
    currentBootCount: Int,
    durationSeconds: Int,
    nowElapsedMillis: Long,
): Long = if (storedBootCount >= 0 && storedBootCount == currentBootCount) {
    storedDeadlineElapsedMillis
} else {
    authorizationFloodWaitDeadline(nowElapsedMillis, durationSeconds)
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
