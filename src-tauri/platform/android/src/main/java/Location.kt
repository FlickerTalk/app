package com.flickertalk.platform

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.pm.PackageManager
import android.location.LocationManager
import android.os.Build
import android.os.CancellationSignal
import android.os.Handler
import android.os.Looper
import androidx.core.content.ContextCompat
import androidx.core.location.LocationManagerCompat
import java.util.concurrent.atomic.AtomicBoolean

// One fix while the app is open. Nothing of it is kept or logged.

enum class LocationAccess { PRECISE, APPROXIMATE, NONE }

const val LOCATION_TIMEOUT_MS = 15_000L

fun locationAccess(fine: Boolean, coarse: Boolean): LocationAccess = when {
    fine -> LocationAccess.PRECISE
    coarse -> LocationAccess.APPROXIMATE
    else -> LocationAccess.NONE
}

// fused (12+) honours "approximate" by itself. GPS needs the fine permission, so approximate asks the network
fun locationProvider(access: LocationAccess, enabled: Set<String>, sdk: Int): String? {
    if (access == LocationAccess.NONE) return null
    if (sdk >= Build.VERSION_CODES.S && LocationManager.FUSED_PROVIDER in enabled) return LocationManager.FUSED_PROVIDER
    val wanted = when (access) {
        LocationAccess.PRECISE -> listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER)
        else -> listOf(LocationManager.NETWORK_PROVIDER)
    }
    return wanted.firstOrNull { it in enabled }
}

// accuracy in metres, at in epoch ms
data class LocationFix(val lat: Double, val lon: Double, val accuracy: Float?, val at: Long)

fun locationPayload(fix: LocationFix?): Map<String, Any> {
    val accuracy = fix?.accuracy ?: return mapOf("found" to false)
    return mapOf("found" to true, "lat" to fix.lat, "lon" to fix.lon, "accuracy" to accuracy.toDouble(), "at" to fix.at)
}

fun holds(activity: Activity, permission: String): Boolean =
    ContextCompat.checkSelfPermission(activity, permission) == PackageManager.PERMISSION_GRANTED

@SuppressLint("MissingPermission") // checked by locationAccess
fun currentFix(activity: Activity, done: (LocationFix?) -> Unit) {
    val once = AtomicBoolean(false)
    val finish = { fix: LocationFix? -> if (once.compareAndSet(false, true)) done(fix) }
    val access = locationAccess(
        fine = holds(activity, Manifest.permission.ACCESS_FINE_LOCATION),
        coarse = holds(activity, Manifest.permission.ACCESS_COARSE_LOCATION),
    )
    val manager = activity.getSystemService(LocationManager::class.java)
    if (manager == null || !LocationManagerCompat.isLocationEnabled(manager)) return finish(null)
    val enabled = manager.getProviders(true).toSet()
    val provider = locationProvider(access, enabled, Build.VERSION.SDK_INT) ?: return finish(null)
    val cancel = CancellationSignal()
    val main = Handler(Looper.getMainLooper())
    main.postDelayed({
        cancel.cancel()
        finish(null)
    }, LOCATION_TIMEOUT_MS)
    try {
        LocationManagerCompat.getCurrentLocation(manager, provider, cancel, ContextCompat.getMainExecutor(activity)) { location ->
            main.removeCallbacksAndMessages(null)
            finish(location?.let { LocationFix(it.latitude, it.longitude, if (it.hasAccuracy()) it.accuracy else null, it.time) })
        }
    } catch (error: Exception) {
        // permission taken back after the check, or the provider went away
        main.removeCallbacksAndMessages(null)
        finish(null)
    }
}
