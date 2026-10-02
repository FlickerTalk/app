package com.flickertalk.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

// The location plugin (2026-10-02): the phone's current position, once, while the app is open.
// No background permission, no following, no map: one fix or nothing within ~15 s.
class LocationTest {
    // The user may answer "approximate" (Android 12+): that is still a position, a rougher one.
    @Test
    fun whatTheUserAllowedIsPreciseApproximateOrNothing() {
        assertEquals(LocationAccess.PRECISE, locationAccess(fine = true, coarse = true))
        assertEquals(LocationAccess.PRECISE, locationAccess(fine = true, coarse = false))
        assertEquals(LocationAccess.APPROXIMATE, locationAccess(fine = false, coarse = true))
        assertEquals(LocationAccess.NONE, locationAccess(fine = false, coarse = false))
    }

    // The fused provider (Android 12+) where there is one; else GPS for a precise fix and the
    // network for an approximate one (GPS needs the fine permission). Nothing on, no provider.
    @Test
    fun theProviderFollowsWhatWasAllowedAndWhatIsOn() {
        val all = setOf("fused", "gps", "network", "passive")
        assertEquals("fused", locationProvider(LocationAccess.PRECISE, all, 31))
        assertEquals("fused", locationProvider(LocationAccess.APPROXIMATE, all, 31))
        assertEquals("gps", locationProvider(LocationAccess.PRECISE, setOf("gps", "network"), 30))
        assertEquals("network", locationProvider(LocationAccess.PRECISE, setOf("network"), 26))
        assertEquals("network", locationProvider(LocationAccess.APPROXIMATE, setOf("gps", "network"), 30))
        assertNull(locationProvider(LocationAccess.APPROXIMATE, setOf("gps"), 29))
        assertNull(locationProvider(LocationAccess.PRECISE, emptySet(), 34))
        assertNull(locationProvider(LocationAccess.NONE, all, 34))
        // Before Android 12 a "fused" entry is not ours to ask for.
        assertEquals("gps", locationProvider(LocationAccess.PRECISE, all, 30))
    }

    // What `currentLocation` resolves with, as Rust reads it (`Located` in platform/src/lib.rs).
    @Test
    fun aFixGoesBackAsFoundWithItsPlaceAndNothingAsNotFound() {
        assertEquals(mapOf<String, Any>("found" to false), locationPayload(null))
        assertEquals(
            mapOf<String, Any>("found" to true, "lat" to 40.41678, "lon" to -3.70379, "accuracy" to 35.0, "at" to 1_790_000_000_000L),
            locationPayload(LocationFix(40.41678, -3.70379, 35f, 1_790_000_000_000L)),
        )
        // A fix that does not say how far off it may be is not shown as if it were exact.
        assertEquals(mapOf<String, Any>("found" to false), locationPayload(LocationFix(40.4, -3.7, null, 1L)))
    }

    @Test
    fun theUserWaitsForAFixAboutFifteenSeconds() {
        assertEquals(15_000L, LOCATION_TIMEOUT_MS)
    }

    // Only "while using the app": no background location, ever. And the location hardware is not
    // required, so Play still offers the app to a phone or tablet without GPS.
    @Test
    fun theManifestAsksForLocationWhileInUseOnlyAndRequiresNoGps() {
        val manifest = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(File("src/main/AndroidManifest.xml"))
        fun names(tag: String) = manifest.getElementsByTagName(tag).let { nodes ->
            (0 until nodes.length).map { nodes.item(it) as Element }
        }
        val permissions = names("uses-permission").map { it.getAttribute("android:name") }
        assertTrue(permissions.contains("android.permission.ACCESS_COARSE_LOCATION"))
        assertTrue(permissions.contains("android.permission.ACCESS_FINE_LOCATION"))
        assertFalse(permissions.contains("android.permission.ACCESS_BACKGROUND_LOCATION"))
        val features = names("uses-feature").associate { it.getAttribute("android:name") to it.getAttribute("android:required") }
        for (feature in listOf("android.hardware.location", "android.hardware.location.gps", "android.hardware.location.network")) {
            assertEquals(feature, "false", features[feature])
        }
    }
}
