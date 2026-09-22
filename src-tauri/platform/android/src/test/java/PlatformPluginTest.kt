package com.flickertalk.platform

import org.junit.Assert.assertEquals
import org.junit.Test

class PlatformPluginTest {
    // Must match `android:authorities` in the plugin's AndroidManifest.xml.
    @Test
    fun theFileProviderAuthorityBelongsToTheApp() {
        assertEquals("com.flickertalk.flickertalk.ft.files", fileProviderAuthority("com.flickertalk.flickertalk"))
    }

    // Android 10 and up write to Downloads without any storage permission.
    @Test
    fun downloadsNeedAndroid10() {
        assertEquals(false, canSaveToDownloads(28))
        assertEquals(true, canSaveToDownloads(29))
    }
}
