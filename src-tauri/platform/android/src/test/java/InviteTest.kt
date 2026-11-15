package com.flickertalk.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

class InviteTest {
    @Test
    fun thePhoneCountryIsTheSimsThenTheNetworksThenTheRegionSetOnThePhone() {
        assertEquals("ES", phoneCountry(sim = "es", network = "fr", region = "GB"))
        assertEquals("FR", phoneCountry(sim = "", network = "fr", region = "GB"))
        assertEquals("GB", phoneCountry(sim = null, network = " ", region = "GB"))
        assertEquals("", phoneCountry(sim = null, network = null, region = ""))
    }

    @Test
    fun aPickedContactGoesBackWithItsNameNumberAndThePhoneCountry() {
        assertEquals(
            mapOf<String, Any>("picked" to true, "name" to "Ana", "number" to "612 345 678", "country" to "ES"),
            pickedContactPayload("Ana", "612 345 678", "ES"),
        )
        assertEquals(
            mapOf<String, Any>("picked" to true, "name" to "", "number" to "", "country" to "ES"),
            pickedContactPayload(null, null, "ES"),
        )
    }

    @Test
    fun aClosedPickerGoesBackAsNobody() {
        assertEquals(mapOf<String, Any>("picked" to false), NOBODY_PICKED)
    }

    @Test
    fun whatsAppIsTriedBeforeWhatsAppBusiness() {
        assertEquals(listOf("com.whatsapp", "com.whatsapp.w4b"), WHATSAPP_PACKAGES)
    }

    @Test
    fun thePluginLeavesTheContactPickerAndWhatsAppToInvite() {
        val plugin = File("src/main/java/PlatformPlugin.kt").readText()
        val invite = File("src/main/java/Invite.kt").readText()
        for (api in listOf("ContactsContract", "TelephonyManager", "ActivityNotFoundException")) {
            assertFalse(api, plugin.contains(api))
            assertTrue(api, invite.contains(api))
        }
    }

    @Test
    fun noManifestAsksForTheContacts() {
        for (path in listOf("src/main/AndroidManifest.xml", "../../gen/android/app/src/main/AndroidManifest.xml")) {
            val nodes = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(File(path)).getElementsByTagName("uses-permission")
            val permissions = (0 until nodes.length).map { (nodes.item(it) as Element).getAttribute("android:name") }
            assertFalse(path, permissions.contains("android.permission.READ_CONTACTS"))
            assertFalse(path, permissions.contains("android.permission.WRITE_CONTACTS"))
        }
    }
}
