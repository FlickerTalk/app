package com.flickertalk.platform

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.browser.customtabs.CustomTabsIntent
import app.tauri.annotation.InvokeArg
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject

const val AUTH_RESULT = "ft.auth.result"

fun isAuthRedirect(uri: String?, scheme: String): Boolean =
    uri != null && scheme.isNotEmpty() && uri.startsWith("$scheme:")

class AuthRedirectActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val launch = packageManager.getLaunchIntentForPackage(packageName)?.apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            putExtra(AUTH_RESULT, intent?.dataString ?: "")
        }
        if (launch != null) startActivity(launch)
        finish()
    }
}

@InvokeArg
class AuthorizeArgs {
    lateinit var url: String
    lateinit var scheme: String
}

class Authorization(private val activity: Activity) {
    private var authWaiting: Invoke? = null
    private var authScheme: String = ""

    // the WebView never sees the login
    fun authorize(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(AuthorizeArgs::class.java)
            authWaiting?.reject("another login started")
            authWaiting = invoke
            authScheme = args.scheme
            CustomTabsIntent.Builder().build().launchUrl(activity, Uri.parse(args.url))
        } catch (error: Exception) {
            authWaiting = null
            invoke.reject(error.message ?: "cannot open the login")
        }
    }

    fun redirected(intent: Intent) {
        val result = intent.getStringExtra(AUTH_RESULT)
        intent.removeExtra(AUTH_RESULT)
        val waiting = authWaiting
        if (waiting != null && isAuthRedirect(result, authScheme)) {
            authWaiting = null
            waiting.resolve(JSObject().apply { put("url", result) })
        }
    }
}
