package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class InCallTest {
    @Test
    fun thePluginLeavesTheCallAudioToInCall() {
        assertFalse(File("src/main/java/PlatformPlugin.kt").readText().contains("AudioFocusRequest"))
        assertTrue(File("src/main/java/InCall.kt").readText().contains("AudioFocusRequest"))
    }
}
