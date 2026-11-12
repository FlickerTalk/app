package com.flickertalk.platform

import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.RingtoneManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.Person

const val CALL_CHANNEL = "ft.call"
const val CALL_NOTIFICATION = 2

fun callTitle(name: String): String? = name.trim().ifEmpty { null }

fun callText(video: Boolean): Int = if (video) R.string.ft_incoming_video_call else R.string.ft_incoming_call

const val RINGING_CALL_CHANNEL = "ft.call.ringing"

internal fun openAppIntent(context: Context, request: Int): PendingIntent? {
    val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
    } ?: return null
    return PendingIntent.getActivity(
        context,
        request,
        launch,
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
}

// a function so the JVM tests don't run Android code when they load this file
private fun ringtoneAttributes(): AudioAttributes = AudioAttributes.Builder()
    .setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE)
    .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
    .build()

fun showCall(context: Context, title: String, text: String, ringing: Boolean = false, timeoutMs: Long = 0, video: Boolean = false) {
    callNotification(context, title, text, ringing, timeoutMs, video)?.let { IncomingCall.post(context, it) }
}

// updating a ringing notification stops Android's insistent ringtone, so only quiet ones get updated
fun callNotification(
    context: Context,
    title: String,
    text: String,
    ringing: Boolean = false,
    timeoutMs: Long = 0,
    video: Boolean = false,
    alertOnce: Boolean = false,
): Notification? {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return null
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        manager.createNotificationChannel(
            NotificationChannel(CALL_CHANNEL, context.getString(R.string.ft_channel_calls), NotificationManager.IMPORTANCE_HIGH).apply {
                setSound(null, null) // we play the ringtone ourselves
                enableVibration(false)
            }
        )
        if (ringing) {
            manager.createNotificationChannel(
                NotificationChannel(RINGING_CALL_CHANNEL, context.getString(R.string.ft_channel_incoming_calls), NotificationManager.IMPORTANCE_HIGH).apply {
                    setSound(RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE), ringtoneAttributes())
                    enableVibration(true)
                    vibrationPattern = RING_PATTERN
                }
            )
        }
    }
    val open = openAppIntent(context, 1) ?: return null
    val answer = tapIntent(context, NotificationTap.CALL_ANSWER)
    val decline = tapIntent(context, NotificationTap.CALL_DECLINE)
    val caller = Person.Builder().setName(title).setImportant(true).build()
    val builder = NotificationCompat.Builder(context, if (ringing) RINGING_CALL_CHANNEL else CALL_CHANNEL)
        .setSmallIcon(R.drawable.ft_notification)
        .setContentTitle(title)
        .setContentText(text)
        .setPriority(NotificationCompat.PRIORITY_MAX)
        .setCategory(NotificationCompat.CATEGORY_CALL)
        .setOngoing(true)
        .setAutoCancel(true)
        .setOnlyAlertOnce(alertOnce)
        .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
        .setContentIntent(open)
        .setFullScreenIntent(open, true)
        .setStyle(NotificationCompat.CallStyle.forIncomingCall(caller, decline, answer).setIsVideo(video))
    if (timeoutMs > 0) builder.setTimeoutAfter(timeoutMs)
    if (ringing) {
        // before Android 8 sound and vibration belong to the notification, not the channel
        builder.setSound(RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE), AudioManager.STREAM_RING)
        builder.setVibrate(RING_PATTERN)
    }
    val notification = builder.build()
    if (ringing) notification.flags = notification.flags or Notification.FLAG_INSISTENT
    return notification
}

fun fullScreenAllowed(context: Context): Boolean {
    val manager = context.getSystemService(NotificationManager::class.java)
    return Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE ||
        manager?.canUseFullScreenIntent() == true
}

fun askForFullScreen(activity: Activity) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
        activity.startActivity(
            Intent(
                android.provider.Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT,
                android.net.Uri.parse("package:" + activity.packageName),
            )
        )
    }
}
