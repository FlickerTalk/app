package com.flickertalk.platform

import android.app.Activity
import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import androidx.core.app.NotificationCompat
import app.tauri.annotation.InvokeArg
import org.json.JSONArray
import org.json.JSONObject

const val REMINDER_CHANNEL = "ft.reminders"
private const val REMINDERS = "reminders"
// lower ids belong to messages and calls
private const val REMINDER_NOTIFICATION_BASE = 1000

data class ReminderEntry(val plugin: String, val id: String, val at: Long, val text: String)

fun parseReminders(json: String): List<ReminderEntry> {
    val entries = mutableListOf<ReminderEntry>()
    val array = try { JSONArray(json) } catch (_: Exception) { return entries }
    for (index in 0 until array.length()) {
        val item = array.optJSONObject(index) ?: continue
        val plugin = item.optString("plugin")
        val id = item.optString("id")
        val at = item.optLong("at", 0)
        if (plugin.isEmpty() || id.isEmpty() || at <= 0) continue
        entries.add(ReminderEntry(plugin, id, at, item.optString("text")))
    }
    return entries
}

fun reminderRequestCode(plugin: String, id: String): Int =
    REMINDER_NOTIFICATION_BASE + ((plugin + "\n" + id).hashCode() and 0x7fffffff) % 1_000_000

fun reminderKey(plugin: String, id: String): String = plugin + "\n" + id

fun pendingReminderOf(value: String?): String = value?.takeIf { it.contains('\n') } ?: ""

// Android 12 asks for exact alarms, 14 denies them by default
fun exactAlarmsAllowed(sdk: Int, canSchedule: () -> Boolean): Boolean =
    sdk < Build.VERSION_CODES.S || canSchedule()

fun opensExactAlarmSettings(sdk: Int, allowed: Boolean): Boolean = sdk >= Build.VERSION_CODES.S && !allowed

// alarms don't survive a reboot, and ones set before exact alarms were allowed have to be set again
fun setsRemindersAgain(action: String?): Boolean =
    action == Intent.ACTION_BOOT_COMPLETED || action == AlarmManager.ACTION_SCHEDULE_EXACT_ALARM_PERMISSION_STATE_CHANGED

fun exactAlarmsAllowed(context: Context): Boolean = exactAlarmsAllowed(Build.VERSION.SDK_INT) {
    Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
        context.getSystemService(AlarmManager::class.java)?.canScheduleExactAlarms() == true
}

fun askForExactAlarms(activity: Activity, allowed: Boolean) {
    if (opensExactAlarmSettings(Build.VERSION.SDK_INT, allowed)) {
        activity.startActivity(
            Intent(android.provider.Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.parse("package:" + activity.packageName))
        )
    }
}

fun remindersStillDue(entries: List<ReminderEntry>, now: Long): List<ReminderEntry> = entries.filter { it.at > now }

private fun reminderChannel(context: Context) {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        manager.createNotificationChannel(
            NotificationChannel(REMINDER_CHANNEL, context.getString(R.string.ft_channel_reminders), NotificationManager.IMPORTANCE_HIGH)
        )
    }
}

private fun reminderIntent(context: Context, entry: ReminderEntry): PendingIntent =
    PendingIntent.getBroadcast(
        context,
        reminderRequestCode(entry.plugin, entry.id),
        Intent(context, ReminderReceiver::class.java).apply {
            putExtra("plugin", entry.plugin)
            putExtra("id", entry.id)
            putExtra("text", entry.text)
        },
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

fun scheduleReminders(context: Context, entries: List<ReminderEntry>) {
    val alarms = context.getSystemService(AlarmManager::class.java) ?: return
    val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
    for (old in parseReminders(preferences.getString(REMINDERS, "[]") ?: "[]")) {
        alarms.cancel(reminderIntent(context, old))
    }
    val exact = exactAlarmsAllowed(context)
    val now = System.currentTimeMillis()
    for (entry in remindersStillDue(entries, now)) {
        val intent = reminderIntent(context, entry)
        try {
            if (exact) alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, entry.at, intent)
            else alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, entry.at, intent)
        } catch (_: SecurityException) {
            alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, entry.at, intent)
        }
    }
    val kept = JSONArray()
    for (entry in entries) {
        kept.put(JSONObject().put("plugin", entry.plugin).put("id", entry.id).put("at", entry.at).put("text", entry.text))
    }
    preferences.edit().putString(REMINDERS, kept.toString()).apply()
}

// no note text on the lock screen unless the core sent one
class ReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val plugin = intent.getStringExtra("plugin") ?: return
        val id = intent.getStringExtra("id") ?: return
        showReminder(context, plugin, id, intent.getStringExtra("text").orEmpty())
    }
}

fun showReminder(context: Context, plugin: String, id: String, text: String) {
    reminderChannel(context)
    val open = tapIntent(context, NotificationTap.REMINDER, reminderRequestCode(plugin, id), reminderKey(plugin, id))
    val notification = NotificationCompat.Builder(context, REMINDER_CHANNEL)
        .setSmallIcon(R.drawable.ft_notification)
        .setContentTitle(context.getString(R.string.ft_reminder))
        .setContentText(text.ifEmpty { context.getString(R.string.ft_reminder_generic) })
        .setPriority(NotificationCompat.PRIORITY_HIGH)
        .setCategory(NotificationCompat.CATEGORY_REMINDER)
        .setAutoCancel(true)
        .setContentIntent(open)
        .build()
    try {
        context.getSystemService(NotificationManager::class.java)?.notify(reminderRequestCode(plugin, id), notification)
    } catch (_: SecurityException) {
        // notifications off, the plugin shows it as due when the app opens
    }
}

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (!setsRemindersAgain(intent.action)) return
        val kept = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString(REMINDERS, "[]") ?: "[]"
        scheduleReminders(context, parseReminders(kept))
    }
}

@InvokeArg
class RemindersArgs {
    lateinit var reminders: String
}
