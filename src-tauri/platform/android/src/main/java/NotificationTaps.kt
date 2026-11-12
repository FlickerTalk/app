package com.flickertalk.platform

import android.app.Activity
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Bundle

// Taps go through FtNotificationTapActivity. As an extra on the launcher intent they got lost when
// the activity was recreated before Tauri registered the plugin. A receiver can't open the app on 12+.

enum class NotificationTap(val action: String) {
    CALL_ANSWER("com.flickertalk.platform.CALL_ANSWER"),
    CALL_DECLINE("com.flickertalk.platform.CALL_DECLINE"),
    CALL_VIDEO("com.flickertalk.platform.CALL_VIDEO"),
    REMINDER("com.flickertalk.platform.REMINDER"),
}

fun notificationTapOf(action: String?): NotificationTap? = NotificationTap.values().firstOrNull { it.action == action }

fun tapCallEvent(tap: NotificationTap): CallEvent? = when (tap) {
    NotificationTap.CALL_ANSWER -> CallEvent.Answer
    NotificationTap.CALL_DECLINE -> CallEvent.Decline
    NotificationTap.CALL_VIDEO -> CallEvent.VideoRequested
    NotificationTap.REMINDER -> null
}

fun tapOpensApp(tap: NotificationTap, coreListens: Boolean): Boolean =
    tap != NotificationTap.CALL_DECLINE || !coreListens

// 0-7 are taken by the other notifications
fun tapRequestCode(tap: NotificationTap): Int = 20 + tap.ordinal

class TappedReminder {
    private var key = ""

    @Synchronized
    fun put(key: String?) {
        this.key = pendingReminderOf(key)
    }

    @Synchronized
    fun take(): String = key.also { key = "" }
}

val tappedReminder = TappedReminder()

private const val REMINDER_KEY = "ft.reminder"

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

fun openApp(context: Context) {
    val launch = context.packageManager.getLaunchIntentForPackage(context.packageName) ?: return
    context.startActivity(launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
}

class FtNotificationTapActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // a recreated instance must not hand the tap over again
        if (savedInstanceState == null) handOver(this, intent)
        finish()
    }
}

fun handOver(context: Context, intent: Intent?) {
    val tap = notificationTapOf(intent?.action) ?: return
    val listening = tapCallEvent(tap)?.let { CallEvents.hears(it) } ?: CallEvents.listening()
    when (tap) {
        NotificationTap.CALL_ANSWER, NotificationTap.CALL_DECLINE -> {
            IncomingCall.dismiss(context)
            CallRinger.silence()
            if (tap == NotificationTap.CALL_ANSWER) {
                // a tap on our notification lets the call service take the microphone type
                InCall.answeredByTap = true
                IncomingCall.answered = true
            }
        }
        NotificationTap.CALL_VIDEO -> InCall.videoByTap = true
        NotificationTap.REMINDER -> tappedReminder.put(intent?.getStringExtra(REMINDER_KEY))
    }
    tapCallEvent(tap)?.let { CallEvents.emit(it) }
    if (tapOpensApp(tap, listening)) openApp(context)
}

// Debug builds only. adb shell am broadcast -n com.flickertalk.app/com.flickertalk.platform.FtDebugNotificationReceiver --es what call
// (call, video-call, or reminder with --es plugin <id> --es id <reminder>)
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
