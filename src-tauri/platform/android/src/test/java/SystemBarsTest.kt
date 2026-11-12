package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class SystemBarsTest {
    @Test
    fun thePluginLeavesTheBarsAppearanceToSystemBars() {
        assertFalse(File("src/main/java/PlatformPlugin.kt").readText().contains("WindowCompat"))
        assertTrue(File("src/main/java/SystemBars.kt").readText().contains("WindowCompat"))
    }
}
