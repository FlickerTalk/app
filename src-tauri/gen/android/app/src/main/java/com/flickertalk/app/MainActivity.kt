package com.flickertalk.app

import android.graphics.Color
import android.os.Bundle
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
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
}
