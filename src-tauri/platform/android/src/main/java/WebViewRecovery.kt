package com.flickertalk.platform

import android.app.Activity
import android.util.Log
import android.webkit.WebView
import java.lang.ref.WeakReference

private const val TAG = "FtWebView"

enum class RendererRecovery { RECREATE, DESTROY_ONLY }

fun rendererRecovery(hasActivity: Boolean, finishing: Boolean, destroyed: Boolean): RendererRecovery =
    if (hasActivity && !finishing && !destroyed) RendererRecovery.RECREATE else RendererRecovery.DESTROY_ONLY

class Latest<T : Any> {
    private var held: WeakReference<T>? = null

    fun adopt(value: T): Boolean {
        val before = held?.get()
        held = WeakReference(value)
        return before != null && before !== value
    }

    fun current(): T? = held?.get()
}

object AppScreen {
    val activity = Latest<Activity>()
    val webView = Latest<WebView>()
}

// a WebView whose renderer is gone can't be used again; recreating the activity is how wry builds a new one
fun recoverFromRendererGone(view: WebView, didCrash: Boolean) {
    val activity = view.context as? Activity
    val recovery = rendererRecovery(activity != null, activity?.isFinishing == true, activity?.isDestroyed == true)
    Log.w(TAG, "renderer gone, crashed=$didCrash, ${recovery.name.lowercase()}")
    // left in its parent: the barcode scanner adds its preview next to the WebView it was loaded with
    view.destroy()
    if (recovery == RendererRecovery.RECREATE) activity?.recreate()
}

fun adoptWebView(activity: Activity, webView: WebView) {
    AppScreen.activity.adopt(activity)
    val replaced = AppScreen.webView.adopt(webView)
    // wry sets its WebViewClient and adds the WebView to the activity only after this
    webView.post {
        serveAssetRanges(webView, activity.filesDir)
        if (!replaced) return@post
        showOverLockScreen(activity, overLockScreen(ringing = IncomingCall.showing, inCall = InCall.active))
        keepScreenOn(activity, InCall.active)
        CallVideoViews.follow(activity, webView)
    }
}
