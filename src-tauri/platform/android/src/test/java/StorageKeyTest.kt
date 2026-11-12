package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class StorageKeyTest {
    @Test
    fun thePluginLeavesTheKeystoreToStorageKey() {
        assertFalse(File("src/main/java/PlatformPlugin.kt").readText().contains("AndroidKeyStore"))
        assertTrue(File("src/main/java/StorageKey.kt").readText().contains("AndroidKeyStore"))
    }
}
