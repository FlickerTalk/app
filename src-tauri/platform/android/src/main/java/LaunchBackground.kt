package com.flickertalk.platform

import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import app.tauri.annotation.InvokeArg

private const val APPEARANCE = "appearance"

fun launchBackground(appearance: String?, systemDark: Boolean): Int = when (appearance) {
    "light" -> Color.WHITE
    "system" -> if (systemDark) Color.BLACK else Color.WHITE
    else -> Color.BLACK
}

fun launchBackground(context: Context): Int {
    val kept = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString(APPEARANCE, null)
    val night = context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK
    return launchBackground(kept, systemDark = night == Configuration.UI_MODE_NIGHT_YES)
}

fun keepAppearance(context: Context, appearance: String) {
    context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().putString(APPEARANCE, appearance).apply()
}

@InvokeArg
class AppearanceArgs {
    var appearance: String = "dark"
}
