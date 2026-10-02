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

// The location plugin (2026-10-02, Plan §53): the phone's current position, once, while the app
// is open, for a plugin the user granted `location` (the core checked before asking here). No
// background permission, no updates after the one fix, and nothing of it is kept or logged.

/** What the user allowed: a precise position, an approximate one (Android 12+), or none. */
enum class LocationAccess { PRECISE, APPROXIMATE, NONE }

/** How long the user waits for a fix before the plugin gets nothing. */
const val LOCATION_TIMEOUT_MS = 15_000L

/** Fine counts as precise even alone; coarse alone is the "approximate" answer of Android 12+. */
fun locationAccess(fine: Boolean, coarse: Boolean): LocationAccess = when {
    fine -> LocationAccess.PRECISE
    coarse -> LocationAccess.APPROXIMATE
    else -> LocationAccess.NONE
}

/**
 * Which provider to ask, out of those that are on: the fused one where Android has it (12+, it
 * honours "approximate" by itself), else GPS for a precise fix (the network if GPS is off) and the
 * network for an approximate one, since GPS needs the fine permission. `null`: nothing to ask.
 */
fun locationProvider(access: LocationAccess, enabled: Set<String>, sdk: Int): String? {
    if (access == LocationAccess.NONE) return null
    if (sdk >= Build.VERSION_CODES.S && LocationManager.FUSED_PROVIDER in enabled) return LocationManager.FUSED_PROVIDER
    val wanted = when (access) {
        LocationAccess.PRECISE -> listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER)
        else -> listOf(LocationManager.NETWORK_PROVIDER)
    }
    return wanted.firstOrNull { it in enabled }
}

/** One fix, as the phone gave it; `accuracy` in metres, `at` in ms since the epoch. */
data class LocationFix(val lat: Double, val lon: Double, val accuracy: Float?, val at: Long)

/**
 * What `currentLocation` resolves with (`Located` in platform/src/lib.rs). A fix that does not
 * say how far off it may be is not passed on as if it were exact.
 */
fun locationPayload(fix: LocationFix?): Map<String, Any> {
    val accuracy = fix?.accuracy ?: return mapOf("found" to false)
    return mapOf("found" to true, "lat" to fix.lat, "lon" to fix.lon, "accuracy" to accuracy.toDouble(), "at" to fix.at)
}

/** Whether this app holds a permission now. */
fun holds(activity: Activity, permission: String): Boolean =
    ContextCompat.checkSelfPermission(activity, permission) == PackageManager.PERMISSION_GRANTED

/**
 * Asks the phone for one current fix and calls `done` once: with the fix, or with `null` if
 * nothing was allowed, nothing is on, or no fix came within [LOCATION_TIMEOUT_MS].
 */
@SuppressLint("MissingPermission") // `locationAccess` checked it; NONE never reaches the manager.
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
        // Taken back between the check and the call, or a provider that went away.
        main.removeCallbacksAndMessages(null)
        finish(null)
    }
}
