package com.flickertalk.app

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import com.flickertalk.platform.captureOpenedLink

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // A link that opened the app (2026-10-08) is handed to the bridge before Tauri starts: the
    // plugin is registered later. Not when the activity is restored or started from recents.
    captureOpenedLink(intent, restored = savedInstanceState != null)
    // The app is dark by default whatever the system theme, so the system bar icons start light.
    // Both bars stay see-through: the app paints under them (the strip under the navigation bar
    // included, theme/base.css). Once the page applies the appearance chosen in Settings, the
    // bridge's `setSystemBars` turns the icons to match (2026-10-02).
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)
  }

  /** A link tapped with the app running, or in the background (`singleTask`). */
  override fun onNewIntent(intent: Intent) {
    captureOpenedLink(intent, restored = false)
    super.onNewIntent(intent)
  }
}
