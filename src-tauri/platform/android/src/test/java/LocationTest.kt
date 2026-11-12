package com.flickertalk.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

class LocationTest {
    @Test
    fun whatTheUserAllowedIsPreciseApproximateOrNothing() {
        assertEquals(LocationAccess.PRECISE, locationAccess(fine = true, coarse = true))
        assertEquals(LocationAccess.PRECISE, locationAccess(fine = true, coarse = false))
        assertEquals(LocationAccess.APPROXIMATE, locationAccess(fine = false, coarse = true))
        assertEquals(LocationAccess.NONE, locationAccess(fine = false, coarse = false))
    }

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
        assertEquals("gps", locationProvider(LocationAccess.PRECISE, all, 30))
    }

    @Test
    fun aFixGoesBackAsFoundWithItsPlaceAndNothingAsNotFound() {
        assertEquals(mapOf<String, Any>("found" to false), locationPayload(null))
        assertEquals(
            mapOf<String, Any>("found" to true, "lat" to 40.41678, "lon" to -3.70379, "accuracy" to 35.0, "at" to 1_790_000_000_000L),
            locationPayload(LocationFix(40.41678, -3.70379, 35f, 1_790_000_000_000L)),
        )
        assertEquals(mapOf<String, Any>("found" to false), locationPayload(LocationFix(40.4, -3.7, null, 1L)))
    }

    @Test
    fun theUserWaitsForAFixAboutFifteenSeconds() {
        assertEquals(15_000L, LOCATION_TIMEOUT_MS)
    }

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
