package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class RingingTest {
    @Test
    fun thePluginLeavesTheRingtoneAndVibrationToRinging() {
        val plugin = File("src/main/java/PlatformPlugin.kt").readText()
        val ringing = File("src/main/java/Ringing.kt").readText()
        for (api in listOf("RingtoneManager", "VibrationEffect")) {
            assertFalse(api, plugin.contains(api))
            assertTrue(api, ringing.contains(api))
        }
    }
}
