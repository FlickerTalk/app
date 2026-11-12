package com.flickertalk.platform

import android.graphics.Color
import android.os.Build
import android.view.Window
import androidx.core.view.WindowCompat
import app.tauri.annotation.InvokeArg

// androidx's scrim, navigation icons can't be dark before Android 8
val NAVIGATION_SCRIM: Int = 0x801B1B1B.toInt()

data class SystemBarsLook(val darkIcons: Boolean, val navigationBarColor: Int)

fun systemBarsLook(dark: Boolean, sdk: Int): SystemBarsLook = SystemBarsLook(
    darkIcons = !dark,
    navigationBarColor = if (!dark && sdk < Build.VERSION_CODES.O) NAVIGATION_SCRIM else Color.TRANSPARENT,
)

fun showSystemBars(window: Window, look: SystemBarsLook) {
    WindowCompat.getInsetsController(window, window.decorView).apply {
        isAppearanceLightStatusBars = look.darkIcons
        isAppearanceLightNavigationBars = look.darkIcons
    }
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
        @Suppress("DEPRECATION")
        window.navigationBarColor = look.navigationBarColor
    }
}

@InvokeArg
class SystemBarsArgs {
    var dark: Boolean = true
}
