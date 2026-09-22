package com.flickertalk.platform

import android.app.ActivityManager
import android.media.AudioManager
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PlatformPluginTest {
    // Must match `android:authorities` in the plugin's AndroidManifest.xml.
    @Test
    fun theFileProviderAuthorityBelongsToTheApp() {
        assertEquals("com.flickertalk.app.ft.files", fileProviderAuthority("com.flickertalk.app"))
    }

    // Android 10 and up write to Downloads without any storage permission.
    @Test
    fun downloadsNeedAndroid10() {
        assertEquals(false, canSaveToDownloads(28))
        assertEquals(true, canSaveToDownloads(29))
    }

    // An incoming call rings as the user set the phone: sound and vibration, vibration only, or
    // nothing at all.
    @Test
    fun ringingFollowsTheRingerMode() {
        assertEquals(Ringing(sound = true, vibrate = true), ringingFor(AudioManager.RINGER_MODE_NORMAL))
        assertEquals(Ringing(sound = false, vibrate = true), ringingFor(AudioManager.RINGER_MODE_VIBRATE))
        assertEquals(Ringing(sound = false, vibrate = false), ringingFor(AudioManager.RINGER_MODE_SILENT))
    }

    // M4: the router's push only says "wake"; anything else is ignored.
    @Test
    fun onlyWakeUpsCount() {
        assertEquals(true, isWake(mapOf("t" to "wake")))
        assertEquals(false, isWake(mapOf("t" to "other")))
        assertEquals(false, isWake(emptyMap()))
    }

    // An open app is already connected: it gets everything without a notification.
    @Test
    fun notifiesOnlyWhenTheAppIsNotOnScreen() {
        assertEquals(false, shouldNotify(ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND))
        assertEquals(true, shouldNotify(ActivityManager.RunningAppProcessInfo.IMPORTANCE_CACHED))
        assertEquals(true, shouldNotify(ActivityManager.RunningAppProcessInfo.IMPORTANCE_SERVICE))
    }

    // The sealed storage key: the 12-byte GCM nonce, then the ciphertext with its tag.
    @Test
    fun aSealedKeySplitsIntoNonceAndCiphertext() {
        val sealed = ByteArray(12) { 1 } + ByteArray(48) { 2 }
        val (iv, ciphertext) = splitSealed(sealed)!!
        assertEquals(12, iv.size)
        assertEquals(48, ciphertext.size)
        assertEquals(null, splitSealed(ByteArray(12)))
    }

    @Test
    fun onlyRealTextIsShared() {
        assertEquals("Add me: https://flickertalk.com/add#card", shareableText("  Add me: https://flickertalk.com/add#card\n"))
        assertNull(shareableText("   "))
    }
}
