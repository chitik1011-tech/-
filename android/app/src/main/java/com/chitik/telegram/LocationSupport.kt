package com.chitik.telegram

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.CancellationSignal
import android.os.Handler
import android.os.Looper
import java.net.URLEncoder
import java.nio.charset.StandardCharsets
import kotlin.math.roundToInt

private const val GEO_VIEW_ACTION = "android.intent.action.VIEW"

data class DeviceGeoPoint(
    val latitude: Double,
    val longitude: Double,
    val accuracyRadiusMeters: Int?,
)

data class GeoIntentSpec(
    val action: String,
    val uri: String,
)

internal fun buildGeoIntentSpec(
    latitude: Double,
    longitude: Double,
    label: String?,
): GeoIntentSpec? {
    if (!latitude.isFinite() || !longitude.isFinite()) return null
    if (latitude !in -90.0..90.0 || longitude !in -180.0..180.0) return null

    val coordinate = "$latitude,$longitude"
    val query = label
        ?.trim()
        ?.takeIf(String::isNotEmpty)
        ?.let {
            val encoded = URLEncoder.encode(it, StandardCharsets.UTF_8.toString())
                .replace("+", "%20")
            "?q=$coordinate($encoded)"
        }
        ?: "?q=$coordinate"

    return GeoIntentSpec(
        action = GEO_VIEW_ACTION,
        uri = "geo:$coordinate$query",
    )
}

internal fun openLocationInMap(
    context: Context,
    latitude: Double,
    longitude: Double,
    label: String?,
): Boolean {
    val spec = buildGeoIntentSpec(latitude, longitude, label) ?: return false
    val intent = Intent(spec.action, Uri.parse(spec.uri))
    return try {
        // Do not preflight with PackageManager.resolveActivity(): package
        // visibility can hide an installed map handler from queries even when
        // Android can resolve this implicit geo intent for startActivity().
        context.startActivity(intent)
        true
    } catch (_: ActivityNotFoundException) {
        false
    } catch (_: SecurityException) {
        false
    }
}

/**
 * Owns a one-shot location request. Cancelling suppresses callbacks and removes
 * the active platform request/listener.
 */
internal class OneShotLocationCallback<T>(
    private val onResult: (T) -> Unit,
) {
    private var completed = false

    @Synchronized
    fun deliver(result: T) {
        if (completed) return
        completed = true
        onResult(result)
    }

    @Synchronized
    fun cancel(): Boolean {
        if (completed) return false
        completed = true
        return true
    }

    @Synchronized
    fun isActive(): Boolean = !completed
}

internal class LocationRequestHandle internal constructor(
    private val cancelRequest: () -> Unit,
) {
    fun cancel() = cancelRequest()
}

/**
 * Requests one current fix only after an explicit user action. Callers cancel
 * the returned handle when the screen leaves or the send action is abandoned.
 */
internal fun requestCurrentLocation(
    context: Context,
    onResult: (Result<DeviceGeoPoint>) -> Unit,
): LocationRequestHandle? {
    val fineGranted =
        context.checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) ==
            PackageManager.PERMISSION_GRANTED
    val coarseGranted =
        context.checkSelfPermission(Manifest.permission.ACCESS_COARSE_LOCATION) ==
            PackageManager.PERMISSION_GRANTED
    if (!fineGranted && !coarseGranted) {
        onResult(Result.failure(SecurityException("Location permission is not granted")))
        return null
    }

    val manager = context.getSystemService(Context.LOCATION_SERVICE) as? LocationManager
    if (manager == null) {
        onResult(Result.failure(IllegalStateException("Location service is unavailable")))
        return null
    }

    val provider = buildList {
        if (fineGranted) add(LocationManager.GPS_PROVIDER)
        add(LocationManager.NETWORK_PROVIDER)
    }.firstOrNull { candidate ->
        runCatching { manager.isProviderEnabled(candidate) }.getOrDefault(false)
    }

    if (provider == null) {
        onResult(Result.failure(IllegalStateException("No location provider is enabled")))
        return null
    }

    val handler = Handler(Looper.getMainLooper())
    val signal = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) CancellationSignal() else null
    var listener: LocationListener? = null
    lateinit var timeout: Runnable

    fun removeRequest() {
        listener?.let { runCatching { manager.removeUpdates(it) } }
        timeout.let { handler.removeCallbacks(it) }
    }

    val callback = OneShotLocationCallback<Result<DeviceGeoPoint>> { result ->
        removeRequest()
        onResult(result)
    }
    fun finish(result: Result<DeviceGeoPoint>) = callback.deliver(result)

    timeout = Runnable {
        finish(Result.failure(IllegalStateException("Timed out waiting for current location")))
    }

    val handle = LocationRequestHandle {
        if (callback.cancel()) {
            signal?.cancel()
            removeRequest()
        }
    }

    val deliver: (Location?) -> Unit = { location ->
        if (location == null || !location.latitude.isFinite() || !location.longitude.isFinite()) {
            finish(Result.failure(IllegalStateException("Current location is unavailable")))
        } else {
            val accuracy = location.accuracy
                .takeIf { location.hasAccuracy() && it.isFinite() && it >= 0f }
                ?.roundToInt()
                ?.takeIf { it in 0..1_500 }
            finish(
                Result.success(
                    DeviceGeoPoint(
                        latitude = location.latitude,
                        longitude = location.longitude,
                        accuracyRadiusMeters = accuracy,
                    ),
                ),
            )
        }
    }

    try {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            manager.getCurrentLocation(provider, signal, context.mainExecutor, deliver)
        } else {
            @Suppress("DEPRECATION")
            val singleUpdateListener = object : LocationListener {
                override fun onLocationChanged(location: Location) = deliver(location)
                @Deprecated("Deprecated in Android")
                override fun onStatusChanged(provider: String?, status: Int, extras: Bundle?) = Unit
                override fun onProviderEnabled(provider: String) = Unit
                override fun onProviderDisabled(provider: String) = Unit
            }
            listener = singleUpdateListener
            @Suppress("DEPRECATION")
            manager.requestSingleUpdate(provider, singleUpdateListener, Looper.getMainLooper())
        }
        if (callback.isActive()) handler.postDelayed(timeout, 20_000L)
    } catch (error: SecurityException) {
        finish(Result.failure(error))
    } catch (error: IllegalArgumentException) {
        finish(Result.failure(error))
    }
    return handle
}
