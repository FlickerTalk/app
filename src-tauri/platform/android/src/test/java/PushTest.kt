package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class PushTest {
    @Test
    fun thePluginLeavesFirebaseToPush() {
        assertFalse(File("src/main/java/PlatformPlugin.kt").readText().contains("com.google.firebase"))
        assertTrue(File("src/main/java/Push.kt").readText().contains("com.google.firebase"))
    }
}
