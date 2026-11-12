package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class FilesTest {
    @Test
    fun thePluginLeavesThePickersViewersAndPrintingToFiles() {
        val plugin = File("src/main/java/PlatformPlugin.kt").readText()
        val files = File("src/main/java/Files.kt").readText()
        for (api in listOf("MediaStore", "FileProvider.getUriForFile", "PrintManager")) {
            assertFalse(api, plugin.contains(api))
            assertTrue(api, files.contains(api))
        }
    }
}
