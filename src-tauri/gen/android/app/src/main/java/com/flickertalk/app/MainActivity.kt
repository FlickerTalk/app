package com.flickertalk.app

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import com.flickertalk.platform.captureOpenedLink

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // before super.onCreate, the plugin registers too late to see the link
    captureOpenedLink(intent, restored = savedInstanceState != null)
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)
  }

  override fun onNewIntent(intent: Intent) {
    captureOpenedLink(intent, restored = false)
    super.onNewIntent(intent)
  }
}
