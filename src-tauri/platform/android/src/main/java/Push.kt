package com.flickertalk.platform

import android.app.Activity
import android.app.ActivityManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import app.tauri.annotation.InvokeArg
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import java.util.concurrent.atomic.AtomicBoolean

// the router's push only ever says "wake", nothing about who or what
fun isWake(data: Map<String, String>): Boolean = data["t"] == "wake"

// slot 0 (or none, from an older router) is the device's own, 1-7 are hidden sessions
fun wakeIsHeard(slot: String?, open: Set<Int>): Boolean {
    if (slot == null || slot == "0") return true
    return slot.toIntOrNull()?.let { it in open } ?: false
}

fun keptSlots(slots: Set<Int>): String = slots.sorted().joinToString(",")

fun slotsKept(kept: String?): Set<Int> =
    kept.orEmpty().split(",").mapNotNull { it.trim().toIntOrNull() }.filter { it in 1..7 }.toSet()

private const val OPEN_SLOTS = "open_slots"

// only slot numbers are kept, never which session
fun openSlotsNow(context: Context): Set<Int> =
    slotsKept(context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString(OPEN_SLOTS, null))

// commit(), not apply(), a push may start a new process right after
fun keepOpenSlots(context: Context, slots: Set<Int>) {
    context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().putString(OPEN_SLOTS, keptSlots(slots)).commit()
}

// closing the prompt makes the page visible again and re-sends the token, which must not ask twice
class NotificationPrompt {
    private val asked = AtomicBoolean(false)

    fun shouldAsk(sdk: Int, granted: Boolean): Boolean = sdk >= 33 && !granted && asked.compareAndSet(false, true)

    companion object {
        val process = NotificationPrompt()
    }
}

fun shouldNotify(importance: Int): Boolean =
    importance > ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND

private const val CHANNEL = "ft.activity"
private const val NOTIFICATION = 1

class FtMessagingService : FirebaseMessagingService() {
    override fun onMessageReceived(message: RemoteMessage) {
        // the router only pushes when our socket is dead, so a live core reconnects now
        val openSlots = openSlotsNow(this)
        if (reconnectsOnPush(message.data, openSlots)) CallEvents.offer(CallEvent.Incoming)
        if (isCall(message.data)) {
            incomingCall(message, openSlots)
            return
        }
        if (!isWake(message.data)) return
        if (!wakeIsHeard(message.data["s"], openSlots)) return
        val state = ActivityManager.RunningAppProcessInfo()
        ActivityManager.getMyMemoryState(state)
        if (shouldNotify(state.importance) && mayDisturbNow(this)) showActivityNotification(this)
    }

    private fun incomingCall(message: RemoteMessage, openSlots: Set<Int>) {
        val state = ActivityManager.RunningAppProcessInfo()
        ActivityManager.getMyMemoryState(state)
        val push = callPush(message.data, openSlots, !shouldNotify(state.importance), mayDisturbNow(this))
        if (push == CallPush.IGNORE) return
        val left = callRingMillis(message.sentTime, System.currentTimeMillis())
        val notification = callNotification(
            this,
            getString(R.string.ft_someone),
            getString(R.string.ft_incoming_call),
            ringing = push == CallPush.RING,
            timeoutMs = left,
        ) ?: return
        IncomingCall.ring(this, notification, left)
        pushLog(this, if (push == CallPush.RING) "call push: ringing" else "call push: quiet")
        PushCore.start(this)
    }

    override fun onNewToken(token: String) {}
}

fun isCall(data: Map<String, String>): Boolean = data["t"] == "call"

enum class CallPush { IGNORE, RING, SILENT }

fun callPush(data: Map<String, String>, open: Set<Int>, appOnScreen: Boolean, mayDisturb: Boolean): CallPush = when {
    !isCall(data) || !wakeIsHeard(data["s"], open) -> CallPush.IGNORE
    appOnScreen -> CallPush.IGNORE
    !mayDisturb -> CallPush.SILENT
    else -> CallPush.RING
}

fun reconnectsOnPush(data: Map<String, String>, open: Set<Int>): Boolean =
    (isCall(data) || isWake(data)) && wakeIsHeard(data["s"], open)

private fun showActivityNotification(context: Context) {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.ft_channel_messages), NotificationManager.IMPORTANCE_HIGH)
        )
    }
    val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
    } ?: return
    val open = PendingIntent.getActivity(context, 0, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    val notification = NotificationCompat.Builder(context, CHANNEL)
        .setSmallIcon(R.drawable.ft_notification)
        .setContentTitle("FlickerTalk")
        .setContentText(context.getString(R.string.ft_something_new))
        .setPriority(NotificationCompat.PRIORITY_HIGH)
        .setCategory(NotificationCompat.CATEGORY_MESSAGE)
        .setAutoCancel(true)
        .setContentIntent(open)
        .build()
    try {
        manager.notify(NOTIFICATION, notification)
    } catch (_: SecurityException) {
        // notifications off, the app gets everything when it opens
    }
}

fun cancelActivityNotification(context: Context) {
    context.getSystemService(NotificationManager::class.java)?.cancel(NOTIFICATION)
}

// week is seven ;-separated days from Monday, each all, none or FROM-TO in minutes of the day
// (past midnight when TO comes first). Empty means no quiet hours.
fun mayDisturb(week: String, day: Int, minute: Int): Boolean {
    if (week.isEmpty()) return true
    val today = week.split(";").getOrNull(day) ?: return true
    return when (today) {
        "all" -> true
        "none" -> false
        else -> {
            val (from, to) = today.split("-").mapNotNull { it.toIntOrNull() }.takeIf { it.size == 2 } ?: return true
            if (from <= to) minute in from until to else minute >= from || minute < to
        }
    }
}

private const val QUIET_HOURS = "quiet_hours"

fun mayDisturbNow(context: Context): Boolean {
    val week = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString(QUIET_HOURS, "") ?: ""
    val now = java.util.Calendar.getInstance()
    val monday = (now.get(java.util.Calendar.DAY_OF_WEEK) + 5) % 7
    return mayDisturb(week, monday, now.get(java.util.Calendar.HOUR_OF_DAY) * 60 + now.get(java.util.Calendar.MINUTE))
}

fun keepQuietHours(context: Context, week: String) {
    context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().putString(QUIET_HOURS, week).apply()
}

fun sendPushToken(invoke: Invoke) {
    try {
        FirebaseMessaging.getInstance().token.addOnCompleteListener { task ->
            if (task.isSuccessful && task.result != null) {
                invoke.resolve(JSObject().apply { put("token", task.result) })
            } else {
                invoke.reject(task.exception?.message ?: "no push token")
            }
        }
    } catch (error: Exception) {
        invoke.reject(error.message ?: "push is not configured")
    }
}

fun askForNotifications(activity: Activity) {
    val granted = ContextCompat.checkSelfPermission(activity, "android.permission.POST_NOTIFICATIONS") == PackageManager.PERMISSION_GRANTED
    if (NotificationPrompt.process.shouldAsk(Build.VERSION.SDK_INT, granted)) {
        activity.requestPermissions(arrayOf("android.permission.POST_NOTIFICATIONS"), 4242)
    }
}

@InvokeArg
class QuietHoursArgs {
    var week: String = ""
}

@InvokeArg
class OpenSlotsArgs {
    var slots: IntArray = IntArray(0)
}
