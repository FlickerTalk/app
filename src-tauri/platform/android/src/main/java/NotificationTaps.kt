package com.flickertalk.platform

import android.app.Activity
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.util.Log

/*
 * Taps on our notifications' buttons (2026-09-29). They used to ride as an extra on the launcher's
 * intent, read by the plugin. With the app's process killed and its task in recents, Android hands
 * that intent to the recreated activity as a new intent before Tauri has registered the plugin
 * (Rust registers it later, off the activity's start): no one read it, and the call was never
 * answered. Now each button opens `FtNotificationTapActivity`, which hands the tap to the process
 * at once (the call events queue, the tapped reminder) and then opens the app if it must. An
 * activity, not a receiver: Android 12+ lets no receiver started by a notification open the app.
 */

/** A button or a tap on one of our notifications, as the action of the intent that carries it. */
enum class NotificationTap(val action: String) {
    CALL_ANSWER("com.flickertalk.platform.CALL_ANSWER"),
    CALL_DECLINE("com.flickertalk.platform.CALL_DECLINE"),
    /** The ongoing call notification's camera. */
    CALL_VIDEO("com.flickertalk.platform.CALL_VIDEO"),
    REMINDER("com.flickertalk.platform.REMINDER"),
}

/** The tap an intent's action names, if it is one of ours. */
fun notificationTapOf(action: String?): NotificationTap? = NotificationTap.values().firstOrNull { it.action == action }

/** What the tap tells the core, as CallKit's buttons do; never the WebView. */
fun tapCallEvent(tap: NotificationTap): CallEvent? = when (tap) {
    NotificationTap.CALL_ANSWER -> CallEvent.Answer
    NotificationTap.CALL_DECLINE -> CallEvent.Decline
    NotificationTap.CALL_VIDEO -> CallEvent.VideoRequested
    NotificationTap.REMINDER -> null
}

/**
 * Whether the tap brings the app to the screen. A decline does not, unless the core is not running:
 * only the core can tell the caller, and it runs with the app (Tauri starts Rust with the activity).
 */
fun tapOpensApp(tap: NotificationTap, coreListens: Boolean): Boolean =
    tap != NotificationTap.CALL_DECLINE || !coreListens

/** Each button's own PendingIntent, clear of the other notifications' request codes (0-7). */
fun tapRequestCode(tap: NotificationTap): Int = 20 + tap.ordinal

/** The `plugin\nid` of the reminder the user tapped, until the app asks for it, once. */
class TappedReminder {
    private var key = ""

    @Synchronized
    fun put(key: String?) {
        this.key = pendingReminderOf(key)
    }

    @Synchronized
    fun take(): String = key.also { key = "" }
}

/** The process's tapped reminder: the tap comes before the plugin exists. */
val tappedReminder = TappedReminder()

/** The extra of a reminder's tap: `plugin\nid`. */
private const val REMINDER_KEY = "ft.reminder"

/** The PendingIntent of a notification's button: the tap's activity, with the tap's action. */
fun tapIntent(context: Context, tap: NotificationTap, request: Int = tapRequestCode(tap), reminder: String? = null): PendingIntent =
    PendingIntent.getActivity(
        context,
        request,
        Intent(context, FtNotificationTapActivity::class.java).apply {
            action = tap.action
            if (reminder != null) putExtra(REMINDER_KEY, reminder)
        },
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

/** The app on the screen, as the launcher opens it: its task comes back as it was. */
fun openApp(context: Context) {
    val launch = context.packageManager.getLaunchIntentForPackage(context.packageName) ?: return
    context.startActivity(launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
}

/**
 * Where the notification's buttons land: it hands the tap over and finishes, before it is ever
 * drawn (translucent, its own task, out of recents). It shows over the lock screen, so declining
 * needs no unlock and answering reaches the core at once.
 */
class FtNotificationTapActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Handed over once: a recreated instance (after the process died) does not tap again.
        if (savedInstanceState == null) handOver(this, intent)
        finish()
    }
}

/** What a tap does, whatever state the app is in: the process has it before this returns. */
fun handOver(context: Context, intent: Intent?) {
    val tap = notificationTapOf(intent?.action) ?: return
    val listening = CallEvents.listening()
    when (tap) {
        NotificationTap.CALL_ANSWER, NotificationTap.CALL_DECLINE -> {
            context.getSystemService(NotificationManager::class.java)?.cancel(CALL_NOTIFICATION)
            CallRinger.silence()
            // The user's own tap: the call's service may take the microphone when it connects.
            if (tap == NotificationTap.CALL_ANSWER) InCall.answeredByTap = true
        }
        // The user's own tap lets the call's service take the camera type.
        NotificationTap.CALL_VIDEO -> InCall.videoByTap = true
        NotificationTap.REMINDER -> tappedReminder.put(intent?.getStringExtra(REMINDER_KEY))
    }
    tapCallEvent(tap)?.let { CallEvents.emit(it) }
    val opens = tapOpensApp(tap, listening)
    if (CALL_DIAGNOSTICS) Log.i("FtCallDiag", "notification tap: ${tap.name.lowercase()}; core listening=$listening; opens app=$opens")
    if (opens) openApp(context)
}

/**
 * Debug builds only (registered in `src/debug/AndroidManifest.xml`): posts our notifications with
 * no real call or alarm, so the taps can be checked on a device.
 * `adb shell am broadcast -n com.flickertalk.app/com.flickertalk.platform.FtDebugNotificationReceiver --es what call`
 * (`call`, `video-call`, or `reminder` with `--es plugin <id> --es id <reminder>`). The call
 * notification is silent: it does not ring.
 */
class FtDebugNotificationReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.getStringExtra("what")) {
            "call", "video-call" -> showCall(
                context,
                context.getString(R.string.ft_someone),
                context.getString(callText(intent.getStringExtra("what") == "video-call")),
                video = intent.getStringExtra("what") == "video-call",
            )
            "reminder" -> showReminder(
                context,
                intent.getStringExtra("plugin") ?: "com.flickertalk.notes",
                intent.getStringExtra("id") ?: "debug",
                "",
            )
        }
    }
}
