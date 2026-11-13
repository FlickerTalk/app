package com.flickertalk.app

import android.content.Intent
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import com.flickertalk.platform.captureOpenedLink
import com.flickertalk.platform.launchBackground

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // before super.onCreate, the plugin registers too late to see the link
    captureOpenedLink(intent, restored = savedInstanceState != null)
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)
    // the WebView arrives after the first frames, and the DayNight theme would paint the window in the system's colours
    window.setBackgroundDrawable(ColorDrawable(launchBackground(this)))
  }

  // a new WebView is white until the page paints
  override fun onWebViewCreate(webView: WebView) {
    webView.setBackgroundColor(launchBackground(this))
  }

  override fun onNewIntent(intent: Intent) {
    captureOpenedLink(intent, restored = false)
    super.onNewIntent(intent)
  }
}
