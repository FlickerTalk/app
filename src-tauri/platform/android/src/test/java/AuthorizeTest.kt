package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class AuthorizeTest {
    @Test
    fun thePluginLeavesTheLoginTabToAuthorize() {
        assertFalse(File("src/main/java/PlatformPlugin.kt").readText().contains("CustomTabsIntent"))
        assertTrue(File("src/main/java/Authorize.kt").readText().contains("CustomTabsIntent"))
    }
}
