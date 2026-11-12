package com.flickertalk.platform

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.Person
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat

// Android 14 refuses the whole service if the mic type is added off screen, unless right after the user's answer
fun microphoneServiceAllowed(granted: Boolean, visible: Boolean, answeredByTap: Boolean): Boolean =
    granted && (visible || answeredByTap)

// inlined constants, only used past sdk checks lint can't follow
@SuppressLint("InlinedApi")
fun callServiceTypes(sdk: Int, microphone: Boolean, camera: Boolean = false): Int {
    if (sdk < Build.VERSION_CODES.Q) return 0
    var types = ServiceInfo.FOREGROUND_SERVICE_TYPE_PHONE_CALL
    if (sdk < Build.VERSION_CODES.R) return types
    if (microphone) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
    if (camera) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA
    return types
}

private const val ONGOING_CALL_NOTIFICATION = 3
const val ONGOING_CALL_CHANNEL = "ft.call.ongoing"
private const val ACTION_HANG_UP = "com.flickertalk.platform.HANG_UP"
private const val ACTION_MUTE = "com.flickertalk.platform.MUTE"

fun ongoingCallNotification(context: Context): Notification {
    val manager = context.getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        manager?.createNotificationChannel(
            NotificationChannel(ONGOING_CALL_CHANNEL, context.getString(R.string.ft_channel_ongoing_calls), NotificationManager.IMPORTANCE_LOW)
        )
    }
    fun action(action: String, request: Int): PendingIntent = PendingIntent.getBroadcast(
        context,
        request,
        Intent(context, FtCallActionReceiver::class.java).setAction(action),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    val title = callTitle(InCall.name) ?: context.getString(R.string.ft_someone)
    val caller = Person.Builder().setName(title).setImportant(true).build()
    val builder = NotificationCompat.Builder(context, ONGOING_CALL_CHANNEL)
        .setSmallIcon(R.drawable.ft_notification)
        .setContentTitle(title)
        .setContentText(context.getString(R.string.ft_ongoing_call))
        .setCategory(NotificationCompat.CATEGORY_CALL)
        .setOngoing(true)
        .setOnlyAlertOnce(true)
        // CallStyle can't switch between voice and video live, so it's published again
        .setStyle(NotificationCompat.CallStyle.forOngoingCall(caller, action(ACTION_HANG_UP, 5)).setIsVideo(InCall.video))
        .addAction(
            R.drawable.ft_notification,
            context.getString(if (InCall.muted) R.string.ft_unmute else R.string.ft_mute),
            action(ACTION_MUTE, 6),
        )
    builder.addAction(R.drawable.ft_notification, context.getString(R.string.ft_video), tapIntent(context, NotificationTap.CALL_VIDEO))
    openAppIntent(context, 4)?.let { builder.setContentIntent(it) }
    return builder.build()
}

class FtCallService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        InCall.service = this
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!InCall.active) {
            stopSelf()
            return START_NOT_STICKY
        }
        promote()
        return START_NOT_STICKY
    }

    fun promote() {
        val recording = ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        val filming = ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
        val microphone = microphoneServiceAllowed(recording, InCall.appVisible, InCall.answeredByTap)
        val camera = cameraServiceAllowed(InCall.video, filming, InCall.appVisible, InCall.videoByTap)
        val sdk = Build.VERSION.SDK_INT
        // One refused type (background, no phone-call leave) fails the whole service, so retry with less, camera first.
        val tries = listOf(
            callServiceTypes(sdk, microphone, camera),
            callServiceTypes(sdk, microphone, false),
            callServiceTypes(sdk, false, false),
        ).distinct()
        for (types in tries) {
            try {
                ServiceCompat.startForeground(this, ONGOING_CALL_NOTIFICATION, ongoingCallNotification(this), types)
                return
            } catch (_: Exception) {
            }
        }
        stopSelf()
    }

    fun refresh() {
        try {
            getSystemService(NotificationManager::class.java)?.notify(ONGOING_CALL_NOTIFICATION, ongoingCallNotification(this))
        } catch (_: SecurityException) {
        }
    }

    override fun onDestroy() {
        if (InCall.service === this) InCall.service = null
        super.onDestroy()
    }
}

class FtCallActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            ACTION_HANG_UP -> {
                CallEvents.emit(CallEvent.End)
                InCall.end(context)
            }
            ACTION_MUTE -> {
                InCall.muted = !InCall.muted
                CallEvents.emit(CallEvent.Mute(InCall.muted))
                InCall.service?.refresh()
            }
        }
    }
}
