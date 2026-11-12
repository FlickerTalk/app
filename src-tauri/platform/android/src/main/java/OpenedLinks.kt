package com.flickertalk.platform

import android.content.Intent

// MainActivity hands links over before Tauri starts, the plugin registers too late to see them.
// Nothing acts on a link here, the page waits for the user's tap.

// Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY
private const val LAUNCHED_FROM_HISTORY = 0x00100000

// from recents, or after the process died, Android hands back the old intent, so don't open its link again
fun openedLinkOf(action: String?, url: String?, flags: Int, restored: Boolean): String? {
    if (restored || action != Intent.ACTION_VIEW) return null
    if (flags and LAUNCHED_FROM_HISTORY != 0) return null
    return url?.takeIf { it.startsWith("https://") }
}

class OpenedLinks {
    private var url = ""
    private var listener: (() -> Unit)? = null

    fun put(url: String) {
        val told = synchronized(this) {
            this.url = url
            listener
        }
        told?.invoke()
    }

    @Synchronized
    fun take(): String = url.also { url = "" }

    @Synchronized
    fun register(listener: () -> Unit) {
        this.listener = listener
    }
}

val openedLinks = OpenedLinks()

fun captureOpenedLink(intent: Intent?, restored: Boolean) {
    val url = openedLinkOf(intent?.action, intent?.dataString, intent?.flags ?: 0, restored) ?: return
    openedLinks.put(url)
}
