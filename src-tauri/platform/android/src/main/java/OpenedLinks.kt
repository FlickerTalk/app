package com.flickertalk.platform

import android.content.Intent

/*
 * Links that open the app (App Links, 2026-10-08): `https://flickertalk.com/add#…` and `/move#…`,
 * verified for this app by `assetlinks.json`. `MainActivity` hands each intent over in `onCreate`
 * and `onNewIntent`, before Tauri runs: Rust registers the plugin later, off the activity's start,
 * so the plugin would miss it (the bug of `NotificationTaps.kt`). The link waits here until the
 * app asks for it (`openedLink`), once; a running app also hears that one came (`LinkOpened`).
 * Nothing acts on it here: Rust says what it is, and the page waits for the user's tap.
 */

/** `Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY`: the activity was started again from recents. */
private const val LAUNCHED_FROM_HISTORY = 0x00100000

/**
 * The link an intent opens the app with: a VIEW of an `https` address the user just opened. An
 * activity started again from recents (or restored from its saved state, after the process died)
 * comes back with its old intent, and must not open that link again.
 */
fun openedLinkOf(action: String?, url: String?, flags: Int, restored: Boolean): String? {
    if (restored || action != Intent.ACTION_VIEW) return null
    if (flags and LAUNCHED_FROM_HISTORY != 0) return null
    return url?.takeIf { it.startsWith("https://") }
}

/** The latest link that opened the app, until the app asks for it, once. */
class OpenedLinks {
    private var url = ""
    private var listener: (() -> Unit)? = null

    /** A link came: kept, and a running app hears of it (it then asks for it). */
    fun put(url: String) {
        val told = synchronized(this) {
            this.url = url
            listener
        }
        told?.invoke()
    }

    @Synchronized
    fun take(): String = url.also { url = "" }

    /** The app listens (`listen_links`): a second listener replaces the first. */
    @Synchronized
    fun register(listener: () -> Unit) {
        this.listener = listener
    }
}

/** The process's opened link: it comes before the plugin exists. */
val openedLinks = OpenedLinks()

/** What `MainActivity` calls with each intent it gets, before Tauri sees it. */
fun captureOpenedLink(intent: Intent?, restored: Boolean) {
    val url = openedLinkOf(intent?.action, intent?.dataString, intent?.flags ?: 0, restored) ?: return
    openedLinks.put(url)
}
