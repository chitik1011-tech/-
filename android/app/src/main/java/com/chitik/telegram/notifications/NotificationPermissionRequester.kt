package com.chitik.telegram.notifications

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat

/**
 * Registers and requests Android 13+ notification permission.
 *
 * Construct this from an Activity before it reaches STARTED (normally in onCreate), then call
 * [request] from an explicit user action such as enabling notifications. It never prompts on its
 * own during startup. On Android versions without runtime notification permission, the callback
 * receives true.
 */
class NotificationPermissionRequester(
    private val activity: ComponentActivity,
    private val onResult: (Boolean) -> Unit,
) {
    private val launcher = activity.registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
        onResult,
    )

    fun request() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || isGranted(activity)) {
            onResult(true)
            return
        }
        launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
    }

    fun isGranted(): Boolean = isGranted(activity)

    companion object {
        fun isGranted(context: Context): Boolean =
            Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
                ContextCompat.checkSelfPermission(
                    context,
                    Manifest.permission.POST_NOTIFICATIONS,
                ) == PackageManager.PERMISSION_GRANTED
    }
}
