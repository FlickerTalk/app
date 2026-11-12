package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class CallServiceTest {
    @Test
    fun thePluginLeavesTheOngoingCallServiceToCallService() {
        assertFalse(File("src/main/java/PlatformPlugin.kt").readText().contains("ServiceCompat"))
        assertTrue(File("src/main/java/CallService.kt").readText().contains("class FtCallService"))
    }
}
