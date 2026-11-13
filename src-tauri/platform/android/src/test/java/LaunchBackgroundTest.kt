package com.flickertalk.platform

import android.graphics.Color
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class LaunchBackgroundTest {
    @Test
    fun aDarkAppStartsBlackWhateverTheSystemIs() {
        assertEquals(Color.BLACK, launchBackground("dark", systemDark = false))
        assertEquals(Color.BLACK, launchBackground("dark", systemDark = true))
    }

    @Test
    fun aLightAppStartsWhiteWhateverTheSystemIs() {
        assertEquals(Color.WHITE, launchBackground("light", systemDark = false))
        assertEquals(Color.WHITE, launchBackground("light", systemDark = true))
    }

    @Test
    fun anAppThatFollowsTheSystemStartsAsTheSystemIsNow() {
        assertEquals(Color.WHITE, launchBackground("system", systemDark = false))
        assertEquals(Color.BLACK, launchBackground("system", systemDark = true))
    }

    @Test
    fun withNothingKeptOrSomethingUnknownItStartsDarkAsTheAppDoes() {
        assertEquals(Color.BLACK, launchBackground(null, systemDark = false))
        assertEquals(Color.BLACK, launchBackground("dim", systemDark = false))
    }

    @Test
    fun theMainActivityPaintsTheWindowAndTheWebViewWithIt() {
        val activity = File("../../gen/android/app/src/main/java/com/flickertalk/app/MainActivity.kt").readText()
        assertTrue(activity.contains("window.setBackgroundDrawable(ColorDrawable(launchBackground(this)))"))
        assertTrue(activity.contains("override fun onWebViewCreate(webView: WebView)"))
        assertTrue(activity.contains("webView.setBackgroundColor(launchBackground(this))"))
    }
}
