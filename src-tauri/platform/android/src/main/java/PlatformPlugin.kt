package com.flickertalk.platform

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.app.Notification
import android.app.Service
import android.content.pm.ApplicationInfo
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.view.WindowManager
import android.os.IBinder
import android.util.Log
import androidx.core.app.ServiceCompat
import androidx.core.view.WindowCompat
import android.graphics.Color
import app.tauri.PermissionState
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.plugin.Channel
import android.app.ActivityManager
import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import org.json.JSONArray
import org.json.JSONObject
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.Ringtone
import android.media.RingtoneManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.graphics.BitmapFactory
import android.os.Bundle
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintDocumentInfo
import android.print.PrintManager
import android.provider.MediaStore
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import android.webkit.WebView
import kotlin.math.roundToInt
import com.android.billingclient.api.AcknowledgePurchaseParams
import com.android.billingclient.api.BillingClient
import com.android.billingclient.api.BillingClientStateListener
import com.android.billingclient.api.BillingFlowParams
import com.android.billingclient.api.BillingResult
import com.android.billingclient.api.PendingPurchasesParams
import com.android.billingclient.api.ProductDetails
import com.android.billingclient.api.Purchase
import com.android.billingclient.api.QueryProductDetailsParams
import com.android.billingclient.api.QueryPurchasesParams
import androidx.core.app.NotificationCompat
import androidx.core.app.Person
import androidx.core.content.ContextCompat
import androidx.print.PrintHelper
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import androidx.core.content.FileProvider
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean

// own subclass, wry already declares androidx's FileProvider and the manifest can't merge two of one class
class FtFileProvider : FileProvider()

const val PICK_LIMIT = 20

fun fileProviderAuthority(packageName: String): String = "$packageName.ft.files"

fun canSaveToDownloads(sdk: Int): Boolean = sdk >= Build.VERSION_CODES.Q

fun usesPhotoPicker(accept: String, sdk: Int): Boolean =
    (accept.startsWith("image/") || accept.startsWith("video/")) && sdk >= Build.VERSION_CODES.TIRAMISU

// null = both
fun photoPickerType(accept: String): String? =
    accept.takeIf { it == "image/*" || it == "video/*" || Regex("^(image|video)/[a-z0-9.+-]+$").matches(it) }

fun documentType(accept: String): String =
    if (Regex("^[a-z]+/([a-z0-9.+-]+|\\*)$").matches(accept)) accept else "*/*"

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

const val CALL_CHANNEL = "ft.call"
const val CALL_NOTIFICATION = 2

fun callTitle(name: String): String? = name.trim().ifEmpty { null }

fun callText(video: Boolean): Int = if (video) R.string.ft_incoming_video_call else R.string.ft_incoming_call

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

fun refusalCancels(coreRinging: Boolean, inCall: Boolean): Boolean = !coreRinging && !inCall

// same 45 s the router gives the push
const val CALL_RING_MS = 45_000L

fun callRingMillis(sentAt: Long, now: Long): Long {
    val elapsed = now - sentAt
    return if (sentAt > 0 && elapsed in 0 until CALL_RING_MS) CALL_RING_MS - elapsed else CALL_RING_MS
}

// mirrors NativeCallEvent in Rust
sealed class CallEvent {
    object Incoming : CallEvent()
    object Answer : CallEvent()
    object End : CallEvent()
    object Decline : CallEvent()
    data class Mute(val muted: Boolean) : CallEvent()
    object AudioActivated : CallEvent()
    object AudioDeactivated : CallEvent()
    data class Visible(val visible: Boolean) : CallEvent()
    data class Orientation(val degrees: Int) : CallEvent()
    object VideoRequested : CallEvent()
    data class Route(val output: CallOutput) : CallEvent()
}

fun callEventPayload(event: CallEvent): Map<String, Any> = when (event) {
    CallEvent.Incoming -> mapOf("event" to "incoming")
    CallEvent.Answer -> mapOf("event" to "answer")
    CallEvent.End -> mapOf("event" to "end")
    CallEvent.Decline -> mapOf("event" to "decline")
    is CallEvent.Mute -> mapOf("event" to "mute", "muted" to event.muted)
    CallEvent.AudioActivated -> mapOf("event" to "audioActivated")
    CallEvent.AudioDeactivated -> mapOf("event" to "audioDeactivated")
    is CallEvent.Visible -> mapOf("event" to "visible", "visible" to event.visible)
    is CallEvent.Orientation -> mapOf("event" to "orientation", "orientation" to event.degrees)
    CallEvent.VideoRequested -> mapOf("event" to "video")
    is CallEvent.Route -> mapOf("event" to "route", "route" to event.output.wire)
}

class CallEventQueue(private val limit: Int = 16) {
    private var sink: ((CallEvent) -> Unit)? = null
    private var pushCore: ((CallEvent) -> Boolean)? = null
    private val waiting = ArrayDeque<CallEvent>()

    @Synchronized
    fun register(sink: (CallEvent) -> Unit) {
        this.sink = sink
        pushCore = null
        while (waiting.isNotEmpty()) sink(waiting.removeFirst())
    }

    @Synchronized
    fun registerPushCore(take: (CallEvent) -> Boolean) {
        if (sink != null) return
        pushCore = take
        val kept = ArrayDeque<CallEvent>()
        while (waiting.isNotEmpty()) {
            val event = waiting.removeFirst()
            if (!toPushCore(event)) kept.addLast(event)
        }
        waiting.addAll(kept)
    }

    @Synchronized
    fun forgetPushCore() {
        pushCore = null
    }

    @Synchronized
    fun emit(event: CallEvent) {
        val listening = sink
        if (listening != null) {
            listening(event)
            return
        }
        if (toPushCore(event)) return
        waiting.addLast(event)
        while (waiting.size > limit) waiting.removeFirst()
    }

    @Synchronized
    fun listening(): Boolean = sink != null

    @Synchronized
    fun hears(event: CallEvent): Boolean = sink != null || (pushCore != null && pushCoreTakes(event))

    @Synchronized
    fun forget() = waiting.clear()

    @Synchronized
    fun offer(event: CallEvent): Boolean {
        val listening = sink
        if (listening != null) {
            listening(event)
            return true
        }
        return toPushCore(event)
    }

    private fun toPushCore(event: CallEvent): Boolean = pushCoreTakes(event) && pushCore?.invoke(event) == true
}

fun reconnectsOnPush(data: Map<String, String>, open: Set<Int>): Boolean =
    (isCall(data) || isWake(data)) && wakeIsHeard(data["s"], open)

fun overLockScreen(ringing: Boolean, inCall: Boolean): Boolean = ringing || inCall

// Android 14 refuses the whole service if the mic type is added off screen, unless right after the user's answer
fun microphoneServiceAllowed(granted: Boolean, visible: Boolean, answeredByTap: Boolean): Boolean =
    granted && (visible || answeredByTap)

fun showOverLockScreen(activity: Activity, on: Boolean) {
    activity.runOnUiThread {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            activity.setShowWhenLocked(on)
            activity.setTurnScreenOn(on)
        } else {
            @Suppress("DEPRECATION")
            val flags = WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON
            if (on) activity.window.addFlags(flags) else activity.window.clearFlags(flags)
        }
    }
}

fun keepScreenOnFlags(inCall: Boolean, add: (Int) -> Unit, clear: (Int) -> Unit) {
    val flag = WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
    if (inCall) add(flag) else clear(flag)
}

// the flag only works while the window is showing
fun keepScreenOn(activity: Activity, inCall: Boolean) {
    activity.runOnUiThread {
        val window = activity.window
        keepScreenOnFlags(inCall, window::addFlags, window::clearFlags)
    }
}

fun modeAfterCall(previous: Int?): Int = when (previous) {
    null, AudioManager.MODE_IN_COMMUNICATION, AudioManager.MODE_IN_CALL -> AudioManager.MODE_NORMAL
    else -> previous
}

const val RINGING_CALL_CHANNEL = "ft.call.ringing"

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

private fun openAppIntent(context: Context, request: Int): PendingIntent? {
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

private const val ONGOING_CALL_NOTIFICATION = 3
const val ONGOING_CALL_CHANNEL = "ft.call.ongoing"
private const val ACTION_HANG_UP = "com.flickertalk.platform.HANG_UP"
private const val ACTION_MUTE = "com.flickertalk.platform.MUTE"

// own thread, so Rust's handler never runs on main and can call the plugin back
object CallEvents {
    private val queue = CallEventQueue()
    private val sender = java.util.concurrent.Executors.newSingleThreadExecutor()

    fun register(channel: Channel) = sender.execute {
        queue.register { channel.sendObject(callEventPayload(it)) }
    }

    fun emit(event: CallEvent) = sender.execute { queue.emit(event) }

    fun offer(event: CallEvent) = sender.execute { queue.offer(event) }

    fun forget() = sender.execute { queue.forget() }

    fun registerPushCore(take: (CallEvent) -> Boolean) = sender.execute { queue.registerPushCore(take) }

    fun forgetPushCore() = sender.execute { queue.forgetPushCore() }

    fun listening(): Boolean = queue.listening()

    fun hears(event: CallEvent): Boolean = queue.hears(event)
}

object CallRinger {
    var ringtone: Ringtone? = null
    var vibrator: Vibrator? = null
    @Volatile
    var coreRinging = false

    fun silence() {
        ringtone?.stop()
        ringtone = null
        vibrator?.cancel()
        vibrator = null
    }
}

data class RingingCall(val title: String, val text: String, val video: Boolean)

// Only posted while the app is off screen, on screen it came down as a banner over our call card.
// Left up when the app comes back, or the full-screen intent would wake the screen on every resume.
class RingingNotice(private val post: (RingingCall) -> Unit, private val cancel: () -> Unit) {
    private var ringing: RingingCall? = null
    private var posted = false

    @Synchronized
    fun ring(call: RingingCall, appVisible: Boolean) {
        ringing = call
        posted = false
        if (appVisible) cancel() else show(call)
    }

    @Synchronized
    fun appHidden() {
        val call = ringing ?: return
        if (!posted) show(call)
    }

    @Synchronized
    fun stop() {
        ringing = null
        posted = false
        cancel()
    }

    private fun show(call: RingingCall) {
        posted = true
        post(call)
    }
}

object InCall {
    var active = false
        private set
    var muted = false
    var name = ""
        private set
    var ringingName = ""
    @Volatile
    var appVisible = false
    @Volatile
    var answeredByTap = false
    @Volatile
    var video = false
    @Volatile
    var videoByTap = false
    private var previousMode: Int? = null
    private var focus: Any? = null
    @Volatile
    var service: FtCallService? = null

    fun start(context: Context, name: String, video: Boolean = false) {
        if (!active) {
            this.name = name
            active = true
            muted = false
            speakerChoice = null
            prefersSpeaker = video || this.video
            newest = null
            reported = null
            CallEvents.forget()
            IncomingCall.dismiss(context)
            takeAudio(context)
        }
        val running = service
        if (running != null) {
            running.promote()
            return
        }
        try {
            ContextCompat.startForegroundService(context, Intent(context, FtCallService::class.java))
        } catch (_: Exception) {
            // Android 12+ refuses a foreground service started from the background. The call still works on screen.
        }
    }

    fun end(context: Context) {
        if (!active) return
        active = false
        muted = false
        answeredByTap = false
        video = false
        videoByTap = false
        context.stopService(Intent(context, FtCallService::class.java))
        giveAudioBack(context)
    }

    private fun takeAudio(context: Context) {
        val audio = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
        previousMode = audio.mode
        audio.mode = AudioManager.MODE_IN_COMMUNICATION
        watchDevices(context.applicationContext, audio)
        route(context)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                        .build()
                )
                .setOnAudioFocusChangeListener {}
                .build()
            audio.requestAudioFocus(request)
            focus = request
        } else {
            @Suppress("DEPRECATION")
            audio.requestAudioFocus(null, AudioManager.STREAM_VOICE_CALL, AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
        }
    }

    private fun giveAudioBack(context: Context) {
        val audio = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            (focus as? AudioFocusRequest)?.let { audio.abandonAudioFocusRequest(it) }
        } else {
            @Suppress("DEPRECATION")
            audio.abandonAudioFocus(null)
        }
        focus = null
        devices?.let { audio.unregisterAudioDeviceCallback(it) }
        devices = null
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) audio.clearCommunicationDevice() else legacy(audio, LegacyRoute.SYSTEM)
        audio.mode = modeAfterCall(previousMode)
        previousMode = null
    }

    private var speakerChoice: Boolean? = null
    private var prefersSpeaker = false
    private var newest: Int? = null
    private var reported: CallOutput? = null
    private var devices: AudioDeviceCallback? = null

    fun setSpeaker(context: Context, on: Boolean) {
        speakerChoice = on
        route(context)
    }

    fun preferSpeaker(context: Context) {
        if (prefersSpeaker) return
        prefersSpeaker = true
        route(context)
    }

    private fun route(context: Context) {
        if (!active) return
        val audio = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
        val output = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val available = audio.availableCommunicationDevices
            val type = callDevice(speakerChoice, prefersSpeaker, available.map { it.type }, newest)
            val device = available.firstOrNull { it.type == type }
            if (device == null || !audio.setCommunicationDevice(device)) audio.clearCommunicationDevice()
            routeLog(context, "device=$type available=${available.map { it.type }} choice=$speakerChoice video=$prefersSpeaker")
            outputOf(audio.communicationDevice?.type ?: type)
        } else {
            val available = audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS).map { it.type }
            val type = callDevice(speakerChoice, prefersSpeaker, available, newest)
            legacy(audio, legacyRoute(type))
            routeLog(context, "legacy device=$type available=$available choice=$speakerChoice video=$prefersSpeaker")
            outputOf(type)
        }
        if (output != reported) {
            reported = output
            CallEvents.emit(CallEvent.Route(output))
        }
    }

    @Suppress("DEPRECATION")
    private fun legacy(audio: AudioManager, route: LegacyRoute) {
        audio.isSpeakerphoneOn = route == LegacyRoute.SPEAKERPHONE
        if (route == LegacyRoute.BLUETOOTH_SCO && !audio.isBluetoothScoOn) {
            audio.startBluetoothSco()
            audio.isBluetoothScoOn = true
        } else if (route != LegacyRoute.BLUETOOTH_SCO && audio.isBluetoothScoOn) {
            audio.isBluetoothScoOn = false
            audio.stopBluetoothSco()
        }
    }

    private fun watchDevices(context: Context, audio: AudioManager) {
        if (devices != null) return
        val callback = object : AudioDeviceCallback() {
            override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) {
                val types = added.map { it.type }
                speakerChoice = choiceAfterDevicesAdded(speakerChoice, types)
                types.lastOrNull(::isHeadset)?.let { newest = it }
                route(context)
            }

            override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) {
                route(context)
            }
        }
        audio.registerAudioDeviceCallback(callback, Handler(Looper.getMainLooper()))
        devices = callback
    }
}

// debuggable builds only, device types only
fun routeLog(context: Context, line: String) {
    if (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) Log.i("FtCallRoute", line)
}

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

const val REMINDER_CHANNEL = "ft.reminders"
private const val REMINDERS = "reminders"
const val AUTH_RESULT = "ft.auth.result"
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

fun isAuthRedirect(uri: String?, scheme: String): Boolean =
    uri != null && scheme.isNotEmpty() && uri.startsWith("$scheme:")

fun opensExactAlarmSettings(sdk: Int, allowed: Boolean): Boolean = sdk >= Build.VERSION_CODES.S && !allowed

// alarms don't survive a reboot, and ones set before exact alarms were allowed have to be set again
fun setsRemindersAgain(action: String?): Boolean =
    action == Intent.ACTION_BOOT_COMPLETED || action == AlarmManager.ACTION_SCHEDULE_EXACT_ALARM_PERMISSION_STATE_CHANGED

fun exactAlarmsAllowed(context: Context): Boolean = exactAlarmsAllowed(Build.VERSION.SDK_INT) {
    Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
        context.getSystemService(AlarmManager::class.java)?.canScheduleExactAlarms() == true
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
class RemindersArgs {
    lateinit var reminders: String
}

@InvokeArg
class AuthorizeArgs {
    lateinit var url: String
    lateinit var scheme: String
}

private const val KEY_ALIAS = "ft.storage"
private const val IV_BYTES = 12

// GCM nonce, then ciphertext + tag
fun splitSealed(sealed: ByteArray): Pair<ByteArray, ByteArray>? =
    if (sealed.size <= IV_BYTES) null else Pair(sealed.copyOfRange(0, IV_BYTES), sealed.copyOfRange(IV_BYTES, sealed.size))

// made once, never leaves the Keystore
private fun keystoreKey(): SecretKey {
    val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    (store.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    generator.init(
        KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .build()
    )
    return generator.generateKey()
}

@InvokeArg
class KeyArgs {
    lateinit var value: String
}

fun sealStorageKey(key: ByteArray): ByteArray {
    val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, keystoreKey()) }
    return cipher.iv + cipher.doFinal(key)
}

fun openStorageKey(sealed: ByteArray): ByteArray {
    val (iv, ciphertext) = splitSealed(sealed) ?: throw IllegalArgumentException("not a sealed key")
    val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply {
        init(Cipher.DECRYPT_MODE, keystoreKey(), GCMParameterSpec(128, iv))
    }
    return cipher.doFinal(ciphertext)
}

data class Ringing(val sound: Boolean, val vibrate: Boolean)

fun ringingFor(ringerMode: Int, quiet: Boolean = false): Ringing = when {
    quiet -> Ringing(sound = false, vibrate = false)
    ringerMode == AudioManager.RINGER_MODE_NORMAL -> Ringing(sound = true, vibrate = true)
    ringerMode == AudioManager.RINGER_MODE_VIBRATE -> Ringing(sound = false, vibrate = true)
    else -> Ringing(sound = false, vibrate = false)
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

private const val PREFERENCES = "ft.platform"
private const val QUIET_HOURS = "quiet_hours"

fun mayDisturbNow(context: Context): Boolean {
    val week = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString(QUIET_HOURS, "") ?: ""
    val now = java.util.Calendar.getInstance()
    val monday = (now.get(java.util.Calendar.DAY_OF_WEEK) + 5) % 7
    return mayDisturb(week, monday, now.get(java.util.Calendar.HOUR_OF_DAY) * 60 + now.get(java.util.Calendar.MINUTE))
}

private val RING_PATTERN = longArrayOf(0, 800, 1200)

@InvokeArg
class RingingArgs {
    var caller: String = ""
    var video: Boolean = false
    var muted: Boolean = false
}

@InvokeArg
class AnsweringArgs {
    var caller: String = ""
    var video: Boolean = false
}

@InvokeArg
class CallEventsArgs {
    lateinit var channel: Channel
}

@InvokeArg
class OutgoingArgs {
    var name: String = ""
    var video: Boolean = false
}

@InvokeArg
class SystemBarsArgs {
    var dark: Boolean = true
}

@InvokeArg
class SpeakerArgs {
    var on: Boolean = false
}

@InvokeArg
class QuietHoursArgs {
    var week: String = ""
}

@InvokeArg
class OpenSlotsArgs {
    var slots: IntArray = IntArray(0)
}

const val PICKED_FOLDER = "uploads"

fun photoName(now: Long, zone: java.util.TimeZone = java.util.TimeZone.getDefault()): String {
    val format = java.text.SimpleDateFormat("'photo-'yyyyMMdd-HHmmss'.jpg'", java.util.Locale.ROOT)
    format.timeZone = zone
    return format.format(java.util.Date(now))
}

// same naming as iOS. A name must never become a path.
fun pickedBase(now: Long, index: Int, name: String): String = "$now-$index-${name.replace('/', '_')}"

// never reuse an existing file, a message may point to it
fun newPickedFile(folder: File, base: String): File {
    val dot = base.lastIndexOf('.').takeIf { it > 0 }
    val (stem, extension) = if (dot == null) base to "" else base.substring(0, dot) to base.substring(dot)
    var attempt = 1
    while (true) {
        val file = File(folder, if (attempt == 1) base else "$stem-$attempt$extension")
        if (file.createNewFile()) return file
        attempt += 1
    }
}

fun keepsPhoto(resultCode: Int, size: Long): Boolean = resultCode == Activity.RESULT_OK && size > 0

fun pickedName(name: String?): String = name?.trim()?.takeIf { it.isNotEmpty() } ?: "file"

fun pickedMime(mime: String?): String = mime?.trim()?.takeIf { it.isNotEmpty() } ?: "application/octet-stream"

fun shareableText(text: String): String? = text.trim().ifEmpty { null }

// androidx's scrim, navigation icons can't be dark before Android 8
val NAVIGATION_SCRIM: Int = 0x801B1B1B.toInt()

// the WebView reads the font scale only when it's created, and the activity isn't recreated any more
fun textZoomFor(fontScale: Float): Int = (fontScale * 100).roundToInt()

data class SystemBarsLook(val darkIcons: Boolean, val navigationBarColor: Int)

fun systemBarsLook(dark: Boolean, sdk: Int): SystemBarsLook = SystemBarsLook(
    darkIcons = !dark,
    navigationBarColor = if (!dark && sdk < Build.VERSION_CODES.O) NAVIGATION_SCRIM else Color.TRANSPARENT,
)

// product and base plan ids as set up in the Play Console
const val YEARLY = "yearly"

const val YEARLY_BASE_PLAN = "yearly-autorenew"

data class StoreOffer(
    val basePlan: String,
    val offerId: String?,
    val token: String,
    val prices: List<String>,
)

fun yearlyOffer(offers: List<StoreOffer>): StoreOffer? {
    val yearly = offers.filter { it.basePlan == YEARLY_BASE_PLAN }
    return yearly.firstOrNull { it.offerId == null } ?: yearly.firstOrNull()
}

fun yearlyPrice(offers: List<StoreOffer>): String? =
    yearlyOffer(offers)?.prices?.lastOrNull()?.trim()?.takeIf { it.isNotEmpty() }

data class StorePurchase(
    val product: String,
    val state: Int,
    val boughtAt: Long,
    val acknowledged: Boolean,
)

const val PLAY_CACHE_MS = 365L * 24 * 60 * 60 * 1000

// Play never gives the client the expiry and the purchase time stays the original one on renewals,
// so a listed subscription counts as a year from this check, refreshed on every check.
fun activeUntil(purchases: List<StorePurchase>, now: Long): Long =
    if (purchases.any { it.product == YEARLY && it.state == Purchase.PurchaseState.PURCHASED }) {
        now + PLAY_CACHE_MS
    } else {
        0L
    }

fun purchaseTrouble(purchases: List<StorePurchase>): String? =
    if (activeUntil(purchases, 0L) == 0L &&
        purchases.any { it.product == YEARLY && it.state == Purchase.PurchaseState.PENDING }
    ) {
        "pending_approval"
    } else {
        null
    }

// Google refunds a purchase that isn't acknowledged within three days
fun needsAcknowledgement(purchase: StorePurchase): Boolean =
    purchase.state == Purchase.PurchaseState.PURCHASED && !purchase.acknowledged

class EntitlementQueue {
    private var sink: ((Long) -> Unit)? = null
    private var latest: Long? = null

    @Synchronized
    fun register(sink: (Long) -> Unit) {
        this.sink = sink
        latest?.let {
            latest = null
            sink(it)
        }
    }

    @Synchronized
    fun offer(until: Long) {
        val sink = sink
        if (sink != null) sink(until) else latest = until
    }
}

object EntitlementEvents {
    private val queue = EntitlementQueue()
    private val sender = java.util.concurrent.Executors.newSingleThreadExecutor()

    fun register(channel: Channel) = sender.execute {
        queue.register { channel.sendObject(mapOf("until" to it)) }
    }

    fun offer(until: Long) = sender.execute { queue.offer(until) }
}

@InvokeArg
class ShareTextArgs {
    lateinit var text: String
}

@InvokeArg
class PickArgs {
    var accept: String = ""
}

@InvokeArg
class OpenFileArgs {
    lateinit var path: String
    lateinit var mime: String
}

@InvokeArg
class SaveFileArgs {
    lateinit var path: String
    lateinit var name: String
    lateinit var mime: String
}

@InvokeArg
class TranslatePairArgs {
    lateinit var from: String
    lateinit var to: String
}

@InvokeArg
class TranslateSourceArgs {
    lateinit var text: String
}

@InvokeArg
class TranslateTextArgs {
    lateinit var text: String
    lateinit var from: String
    lateinit var to: String
}

@TauriPlugin(
    permissions = [
        Permission(strings = [Manifest.permission.RECORD_AUDIO], alias = "microphone"),
        Permission(strings = [Manifest.permission.CAMERA], alias = "camera"),
        // both, so the user can pick precise or approximate
        Permission(strings = [Manifest.permission.ACCESS_COARSE_LOCATION, Manifest.permission.ACCESS_FINE_LOCATION], alias = "location"),
    ],
)
class PlatformPlugin(private val activity: Activity) : Plugin(activity) {
    private var authWaiting: Invoke? = null
    private var authScheme: String = ""

    private var webView: WebView? = null

    private val ringingNotice = RingingNotice(
        post = { call -> showCall(activity, call.title, call.text, video = call.video) },
        cancel = { IncomingCall.dismiss(activity) },
    )

    override fun load(webView: WebView) {
        super.load(webView)
        this.webView = webView
        // wry has set its WebViewClient by now, so it can be wrapped
        activity.runOnUiThread { serveAssetRanges(webView, activity.filesDir) }
        InCall.appVisible = true
        activity.getSystemService(NotificationManager::class.java)?.cancel(NOTIFICATION)
        if (IncomingCall.showing) showOverLockScreen(activity, overLockScreen(ringing = true, inCall = InCall.active))
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        webView?.settings?.textZoom = textZoomFor(newConfig.fontScale)
    }

    override fun onResume() {
        super.onResume()
        InCall.appVisible = true
        CallEvents.offer(CallEvent.Visible(true))
        if (InCall.active) InCall.service?.promote()
        tellEntitlement()
    }

    override fun onPause() {
        super.onPause()
        InCall.appVisible = false
        CallEvents.offer(CallEvent.Visible(false))
        ringingNotice.appHidden()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        val result = intent.getStringExtra(AUTH_RESULT)
        intent.removeExtra(AUTH_RESULT)
        val waiting = authWaiting
        if (waiting != null && isAuthRedirect(result, authScheme)) {
            authWaiting = null
            waiting.resolve(JSObject().apply { put("url", result) })
        }
    }

    @Command
    fun pendingReminder(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("reminder", tappedReminder.take()) })
    }

    @Command
    fun openedLink(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("url", openedLinks.take()) })
    }

    @Command
    fun registerLinkEvents(invoke: Invoke) {
        try {
            val channel = invoke.parseArgs(CallEventsArgs::class.java).channel
            openedLinks.register { channel.sendObject(mapOf("event" to "linkOpened")) }
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "no channel")
        }
    }

    @Command
    fun setReminders(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(RemindersArgs::class.java)
            scheduleReminders(activity, parseReminders(args.reminders))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot set the reminders")
        }
    }

    @Command
    fun exactAlarms(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("allowed", exactAlarmsAllowed(activity)) })
    }

    @Command
    fun requestExactAlarms(invoke: Invoke) {
        val allowed = exactAlarmsAllowed(activity)
        try {
            if (opensExactAlarmSettings(Build.VERSION.SDK_INT, allowed)) {
                activity.startActivity(
                    Intent(android.provider.Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.parse("package:" + activity.packageName))
                )
            }
            invoke.resolve(JSObject().apply { put("allowed", allowed) })
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot open the setting")
        }
    }

    // the WebView never sees the login
    @Command
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

    @Command
    fun callAnswering(invoke: Invoke) {
        val args = invoke.parseArgs(AnsweringArgs::class.java)
        InCall.ringingName = args.caller
        CallRinger.coreRinging = false
        ringingNotice.stop()
        silence()
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = true))
        invoke.resolve()
    }

    @Command
    fun registerCallEvents(invoke: Invoke) {
        try {
            CallEvents.register(invoke.parseArgs(CallEventsArgs::class.java).channel)
            CallEvents.offer(CallEvent.Visible(InCall.appVisible))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "no channel")
        }
    }

    @Command
    fun callStartedOutgoing(invoke: Invoke) {
        val args = invoke.parseArgs(OutgoingArgs::class.java)
        activity.runOnUiThread { InCall.start(activity, args.name, args.video) }
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = true))
        keepScreenOn(activity, inCall = true)
        invoke.resolve()
    }

    @Command
    fun callConnected(invoke: Invoke) {
        activity.runOnUiThread { InCall.start(activity, InCall.ringingName) }
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = true))
        keepScreenOn(activity, inCall = true)
        invoke.resolve()
    }

    @Command
    fun callEnded(invoke: Invoke) {
        activity.runOnUiThread { InCall.end(activity) }
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = false))
        keepScreenOn(activity, inCall = false)
        invoke.resolve()
    }

    @Command
    fun setSystemBars(invoke: Invoke) {
        val args = invoke.parseArgs(SystemBarsArgs::class.java)
        val look = systemBarsLook(args.dark, Build.VERSION.SDK_INT)
        activity.runOnUiThread {
            val window = activity.window
            WindowCompat.getInsetsController(window, window.decorView).apply {
                isAppearanceLightStatusBars = look.darkIcons
                isAppearanceLightNavigationBars = look.darkIcons
            }
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
                @Suppress("DEPRECATION")
                window.navigationBarColor = look.navigationBarColor
            }
            invoke.resolve()
        }
    }

    @Command
    fun setSpeaker(invoke: Invoke) {
        val args = invoke.parseArgs(SpeakerArgs::class.java)
        activity.runOnUiThread { InCall.setSpeaker(activity, args.on) }
        invoke.resolve()
    }

    @Command
    fun requestMicrophone(invoke: Invoke) {
        if (getPermissionState("microphone") == PermissionState.GRANTED) {
            invoke.resolve(JSObject().apply { put("granted", true) })
        } else {
            requestPermissionForAlias("microphone", invoke, "microphoneAnswered")
        }
    }

    @PermissionCallback
    fun microphoneAnswered(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("granted", getPermissionState("microphone") == PermissionState.GRANTED) })
    }

    @Command
    fun currentLocation(invoke: Invoke) {
        val allowed = holds(activity, Manifest.permission.ACCESS_FINE_LOCATION) || holds(activity, Manifest.permission.ACCESS_COARSE_LOCATION)
        if (allowed) {
            locate(invoke)
        } else {
            requestPermissionForAlias("location", invoke, "locationAnswered")
        }
    }

    @PermissionCallback
    fun locationAnswered(invoke: Invoke) {
        locate(invoke)
    }

    private fun locate(invoke: Invoke) {
        activity.runOnUiThread {
            currentFix(activity) { fix ->
                invoke.resolve(JSObject().apply { locationPayload(fix).forEach { (key, value) -> put(key, value) } })
            }
        }
    }

    // rejections carry the error kind, never the text
    @Command
    fun translateLanguages(invoke: Invoke) = translating(invoke) {
        invoke.resolve(JSObject().apply { put("languages", JSArray(supportedTags())) })
    }

    @Command
    fun translateDetect(invoke: Invoke) = translating(invoke) {
        val args = invoke.parseArgs(TranslateSourceArgs::class.java)
        detectLanguage(args.text) { tag -> invoke.resolve(jsObject(detectPayload(tag))) }
    }

    @Command
    fun translateStatus(invoke: Invoke) = translating(invoke) {
        val args = invoke.parseArgs(TranslatePairArgs::class.java)
        pairStatus(args.from, args.to) { status ->
            if (status == null) invoke.reject("failed: models") else invoke.resolve(JSObject().apply { put("status", status) })
        }
    }

    @Command
    fun translatePrepare(invoke: Invoke) = translating(invoke) {
        val args = invoke.parseArgs(TranslatePairArgs::class.java)
        preparePair(activity, args.from, args.to) { ready -> invoke.resolve(JSObject().apply { put("ready", ready) }) }
    }

    @Command
    fun translateRemove(invoke: Invoke) = translating(invoke) {
        val args = invoke.parseArgs(TranslatePairArgs::class.java)
        removePair(args.from, args.to) { removed -> invoke.resolve(JSObject().apply { put("removed", removed) }) }
    }

    @Command
    fun translateText(invoke: Invoke) = translating(invoke) {
        val args = invoke.parseArgs(TranslateTextArgs::class.java)
        translateOnPhone(args.text, args.from, args.to) { text -> invoke.resolve(jsObject(textPayload(text))) }
    }

    private fun translating(invoke: Invoke, work: () -> Unit) {
        try {
            work()
        } catch (error: Exception) {
            invoke.reject("failed: ${error.javaClass.simpleName}")
        }
    }

    private fun jsObject(payload: Map<String, Any>) = JSObject().apply { payload.forEach { (key, value) -> put(key, value) } }

    @Command
    fun requestCamera(invoke: Invoke) {
        when (cameraRequest(getPermissionState("camera"))) {
            CameraRequest.GRANTED -> invoke.resolve(JSObject().apply { put("granted", true) })
            CameraRequest.DENIED -> invoke.resolve(JSObject().apply { put("granted", false) })
            CameraRequest.ASK -> requestPermissionForAlias("camera", invoke, "cameraAnswered")
        }
    }

    @PermissionCallback
    fun cameraAnswered(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("granted", getPermissionState("camera") == PermissionState.GRANTED) })
    }

    @Command
    fun attachVideo(invoke: Invoke) {
        val web = webView
        if (web == null) {
            invoke.reject("no webview")
            return
        }
        activity.runOnUiThread {
            CallVideoViews.attach(activity, web)
            invoke.resolve()
        }
    }

    @Command
    fun videoLayout(invoke: Invoke) {
        val args = invoke.parseArgs(VideoLayoutArgs::class.java)
        activity.runOnUiThread {
            CallVideoViews.layout(args)
            invoke.resolve()
        }
    }

    @Command
    fun videoShape(invoke: Invoke) {
        val args = invoke.parseArgs(VideoShapeArgs::class.java)
        activity.runOnUiThread {
            CallVideoViews.shape(args.width, args.height)
            invoke.resolve()
        }
    }

    @Command
    fun detachVideo(invoke: Invoke) {
        activity.runOnUiThread {
            CallVideoViews.detach()
            invoke.resolve()
        }
    }

    @Command
    fun callVideo(invoke: Invoke) {
        val on = invoke.parseArgs(CallVideoArgs::class.java).on
        activity.runOnUiThread {
            if (InCall.video != on) {
                InCall.video = on
                if (!on) InCall.videoByTap = false
                if (InCall.active) InCall.service?.promote()
            }
            if (on && InCall.active) InCall.preferSpeaker(activity)
            invoke.resolve()
        }
    }

    @Command
    fun canShowFullScreen(invoke: Invoke) {
        val manager = activity.getSystemService(NotificationManager::class.java)
        val allowed = Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE ||
            manager?.canUseFullScreenIntent() == true
        invoke.resolve(JSObject().apply { put("allowed", allowed) })
    }

    @Command
    fun askFullScreen(invoke: Invoke) {
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                activity.startActivity(
                    Intent(
                        android.provider.Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT,
                        android.net.Uri.parse("package:" + activity.packageName),
                    )
                )
            }
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot open the setting")
        }
    }

    @Command
    fun startRinging(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(RingingArgs::class.java)
            InCall.ringingName = args.caller
            CallRinger.coreRinging = true
            showOverLockScreen(activity, overLockScreen(ringing = true, inCall = InCall.active))
            silence()
            ringingNotice.ring(
                RingingCall(
                    title = callTitle(args.caller) ?: activity.getString(R.string.ft_someone),
                    text = activity.getString(callText(args.video)),
                    video = args.video,
                ),
                appVisible = InCall.appVisible,
            )
            val audio = activity.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            val ringing = ringingFor(audio.ringerMode, quiet = args.muted || !mayDisturbNow(activity))
            if (ringing.sound) {
                val uri = RingtoneManager.getActualDefaultRingtoneUri(activity, RingtoneManager.TYPE_RINGTONE)
                    ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE)
                CallRinger.ringtone = RingtoneManager.getRingtone(activity, uri)?.apply {
                    audioAttributes = AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build()
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) isLooping = true
                    play()
                }
            }
            if (ringing.vibrate && Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                CallRinger.vibrator = phoneVibrator()?.apply { vibrate(VibrationEffect.createWaveform(RING_PATTERN, 0)) }
            }
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot ring")
        }
    }

    @Command
    fun sealKey(invoke: Invoke) {
        try {
            val key = Base64.decode(invoke.parseArgs(KeyArgs::class.java).value, Base64.NO_WRAP)
            val sealed = sealStorageKey(key)
            invoke.resolve(JSObject().apply { put("value", Base64.encodeToString(sealed, Base64.NO_WRAP)) })
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot seal the key")
        }
    }

    @Command
    fun forgetKey(invoke: Invoke) {
        try {
            KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.deleteEntry(KEY_ALIAS)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot forget the key")
        }
    }

    @Command
    fun openKey(invoke: Invoke) {
        try {
            val sealed = Base64.decode(invoke.parseArgs(KeyArgs::class.java).value, Base64.NO_WRAP)
            invoke.resolve(JSObject().apply { put("value", Base64.encodeToString(openStorageKey(sealed), Base64.NO_WRAP)) })
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot open the key")
        }
    }

    @Command
    fun pushToken(invoke: Invoke) {
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

    @Command
    fun requestNotifications(invoke: Invoke) {
        val granted = ContextCompat.checkSelfPermission(activity, "android.permission.POST_NOTIFICATIONS") == PackageManager.PERMISSION_GRANTED
        if (NotificationPrompt.process.shouldAsk(Build.VERSION.SDK_INT, granted)) {
            activity.requestPermissions(arrayOf("android.permission.POST_NOTIFICATIONS"), 4242)
        }
        invoke.resolve()
    }

    @Command
    fun restartApp(invoke: Invoke) {
        val launch = activity.packageManager.getLaunchIntentForPackage(activity.packageName)
        if (launch?.component == null) {
            invoke.reject("no launcher activity")
            return
        }
        invoke.resolve()
        activity.startActivity(Intent.makeRestartActivityTask(launch.component))
        Runtime.getRuntime().exit(0)
    }

    @Command
    fun setOpenSlots(invoke: Invoke) {
        keepOpenSlots(activity, invoke.parseArgs(OpenSlotsArgs::class.java).slots.toSet())
        invoke.resolve()
    }

    @Command
    fun setQuietHours(invoke: Invoke) {
        val week = invoke.parseArgs(QuietHoursArgs::class.java).week
        activity.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().putString(QUIET_HOURS, week).apply()
        invoke.resolve()
    }

    @Command
    fun callRefused(invoke: Invoke) {
        if (refusalCancels(CallRinger.coreRinging, InCall.active)) {
            IncomingCall.dismiss(activity)
            silence()
        }
        invoke.resolve()
    }

    @Command
    fun stopRinging(invoke: Invoke) {
        CallRinger.coreRinging = false
        ringingNotice.stop()
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = InCall.active))
        silence()
        invoke.resolve()
    }

    private fun silence() = CallRinger.silence()

    private fun phoneVibrator(): Vibrator? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            activity.getSystemService(VibratorManager::class.java)?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            activity.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        }

    @Command
    fun shareText(invoke: Invoke) {
        try {
            val text = shareableText(invoke.parseArgs(ShareTextArgs::class.java).text)
            if (text == null) {
                invoke.reject("nothing to share")
                return
            }
            val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
            activity.startActivity(Intent.createChooser(send, null))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot share")
        }
    }

    // the WebView's file input can't be left without picking something, the system picker can
    @Command
    fun pickFiles(invoke: Invoke) {
        val accept = try {
            invoke.parseArgs(PickArgs::class.java).accept
        } catch (_: Exception) {
            ""
        }
        val intent = if (usesPhotoPicker(accept, Build.VERSION.SDK_INT)) {
            Intent(MediaStore.ACTION_PICK_IMAGES).apply {
                photoPickerType(accept)?.let { type = it }
                putExtra(MediaStore.EXTRA_PICK_IMAGES_MAX, PICK_LIMIT)
            }
        } else {
            Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = documentType(accept)
                putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
            }
        }
        startActivityForResult(invoke, intent, "picked")
    }

    // with CAMERA declared (for calls) the capture intent is refused until it's granted
    @Command
    fun takePhoto(invoke: Invoke) {
        if (ContextCompat.checkSelfPermission(activity, android.Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            activity.requestPermissions(arrayOf(android.Manifest.permission.CAMERA), 4243)
            invoke.resolve(JSObject().apply { put("files", JSArray()) })
            return
        }
        val folder = File(activity.filesDir, PICKED_FOLDER).apply { mkdirs() }
        val target = newPickedFile(folder, photoName(System.currentTimeMillis()))
        pendingPhoto = target
        val uri = FileProvider.getUriForFile(activity, fileProviderAuthority(activity.packageName), target)
        val intent = Intent(MediaStore.ACTION_IMAGE_CAPTURE)
            .putExtra(MediaStore.EXTRA_OUTPUT, uri)
            .addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION)
        startActivityForResult(invoke, intent, "photoTaken")
    }

    private var pendingPhoto: File? = null

    @ActivityCallback
    fun photoTaken(invoke: Invoke, result: ActivityResult) {
        val target = pendingPhoto
        pendingPhoto = null
        val files = JSArray()
        if (target != null) {
            if (keepsPhoto(result.resultCode, target.length())) {
                files.put(JSObject().apply {
                    put("path", target.absolutePath)
                    put("name", target.name)
                    put("mime", "image/jpeg")
                    put("size", target.length())
                })
            } else {
                target.delete()
            }
        }
        invoke.resolve(JSObject().apply { put("files", files) })
    }

    @ActivityCallback
    fun picked(invoke: Invoke, result: ActivityResult) {
        val picked = JSArray()
        try {
            val data = result.data
            val uris = mutableListOf<android.net.Uri>()
            data?.clipData?.let { clip -> for (index in 0 until clip.itemCount) uris.add(clip.getItemAt(index).uri) }
            data?.data?.let(uris::add)
            val now = System.currentTimeMillis()
            for ((index, uri) in uris.withIndex()) {
                copyIn(uri, now, index)?.let(picked::put)
            }
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot read what was picked")
            return
        }
        invoke.resolve(JSObject().apply { put("files", picked) })
    }

    private fun copyIn(uri: android.net.Uri, now: Long, index: Int): JSObject? {
        val resolver = activity.contentResolver
        var name = "file"
        var size = 0L
        resolver.query(uri, null, null, null, null)?.use { cursor ->
            val nameColumn = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
            val sizeColumn = cursor.getColumnIndex(android.provider.OpenableColumns.SIZE)
            if (cursor.moveToFirst()) {
                if (nameColumn >= 0) name = pickedName(cursor.getString(nameColumn))
                if (sizeColumn >= 0 && !cursor.isNull(sizeColumn)) size = cursor.getLong(sizeColumn)
            }
        }
        val folder = File(activity.filesDir, PICKED_FOLDER).apply { mkdirs() }
        val input = resolver.openInputStream(uri) ?: return null
        val target = newPickedFile(folder, pickedBase(now, index, name))
        input.use { target.outputStream().use { output -> it.copyTo(output) } }
        return JSObject().apply {
            put("path", target.absolutePath)
            put("name", name)
            put("mime", pickedMime(resolver.getType(uri)))
            put("size", if (size > 0) size else target.length())
        }
    }

    @Command
    fun openFile(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(OpenFileArgs::class.java)
            val uri = FileProvider.getUriForFile(activity, fileProviderAuthority(activity.packageName), File(args.path))
            val view = Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, args.mime)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            activity.startActivity(Intent.createChooser(view, null))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot open the file")
        }
    }

    @Command
    fun shareFile(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SaveFileArgs::class.java)
            val uri = FileProvider.getUriForFile(activity, fileProviderAuthority(activity.packageName), File(args.path))
            val send = Intent(Intent.ACTION_SEND)
                .setType(args.mime)
                .putExtra(Intent.EXTRA_STREAM, uri)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            activity.startActivity(Intent.createChooser(send, null))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot share the file")
        }
    }

    // rejections are plan.trouble.* keys, the app translates them
    private var billing: BillingClient? = null

    private var buying: Invoke? = null

    private fun withBilling(invoke: Invoke, work: (BillingClient) -> Unit) =
        withBilling({ invoke.reject("store_unavailable") }, work)

    private fun withBilling(unavailable: () -> Unit, work: (BillingClient) -> Unit) {
        val client = billing ?: BillingClient.newBuilder(activity)
            .enableAutoServiceReconnection()
            .enablePendingPurchases(PendingPurchasesParams.newBuilder().enableOneTimeProducts().build())
            .setListener { result, purchases -> purchasesUpdated(result, purchases) }
            .build()
            .also { billing = it }
        if (client.isReady) {
            work(client)
            return
        }
        client.startConnection(object : BillingClientStateListener {
            override fun onBillingSetupFinished(result: BillingResult) {
                if (result.responseCode == BillingClient.BillingResponseCode.OK) {
                    work(client)
                } else {
                    unavailable()
                }
            }

            override fun onBillingServiceDisconnected() {}
        })
    }

    private fun storePurchases(purchases: List<Purchase>): List<StorePurchase> =
        purchases.flatMap { purchase ->
            purchase.products.map {
                StorePurchase(it, purchase.purchaseState, purchase.purchaseTime, purchase.isAcknowledged)
            }
        }

    private fun acknowledge(client: BillingClient, purchases: List<Purchase>) {
        for (purchase in purchases) {
            val ours = storePurchases(listOf(purchase)).filter { it.product == YEARLY }
            if (ours.any { needsAcknowledgement(it) }) {
                client.acknowledgePurchase(
                    AcknowledgePurchaseParams.newBuilder().setPurchaseToken(purchase.purchaseToken).build(),
                ) {}
            }
        }
    }

    private fun purchasesUpdated(result: BillingResult, purchases: List<Purchase>?) {
        val invoke = buying
        if (invoke == null) {
            if (result.responseCode == BillingClient.BillingResponseCode.OK) {
                billing?.let { acknowledge(it, purchases.orEmpty()) }
                tellEntitlement()
            }
            return
        }
        buying = null
        when (result.responseCode) {
            BillingClient.BillingResponseCode.OK -> {
                val bought = purchases.orEmpty()
                billing?.let { acknowledge(it, bought) }
                val trouble = purchaseTrouble(storePurchases(bought))
                if (trouble != null) {
                    invoke.reject(trouble)
                } else {
                    invoke.resolve(JSObject().apply { put("until", activeUntil(storePurchases(bought), System.currentTimeMillis())) })
                }
            }
            BillingClient.BillingResponseCode.USER_CANCELED -> invoke.reject("cancelled")
            else -> invoke.reject("payment_failed")
        }
    }

    private fun yearlyQuery(): QueryProductDetailsParams =
        QueryProductDetailsParams.newBuilder()
            .setProductList(
                listOf(
                    QueryProductDetailsParams.Product.newBuilder()
                        .setProductId(YEARLY)
                        .setProductType(BillingClient.ProductType.SUBS)
                        .build(),
                ),
            )
            .build()

    private fun storeOffers(details: ProductDetails?): List<StoreOffer> =
        details?.subscriptionOfferDetails.orEmpty().map { offer ->
            StoreOffer(
                offer.basePlanId,
                offer.offerId,
                offer.offerToken,
                offer.pricingPhases.pricingPhaseList.map { it.formattedPrice },
            )
        }

    @Command
    fun subscriptionPrice(invoke: Invoke) {
        withBilling(invoke) { client ->
            client.queryProductDetailsAsync(yearlyQuery()) { result, found ->
                val details = found.productDetailsList.firstOrNull { it.productId == YEARLY }
                val price = if (result.responseCode == BillingClient.BillingResponseCode.OK) {
                    yearlyPrice(storeOffers(details))
                } else {
                    null
                }
                invoke.resolve(JSObject().apply { price?.let { put("price", it) } })
            }
        }
    }

    @Command
    fun subscribe(invoke: Invoke) {
        withBilling(invoke) { client ->
            client.queryProductDetailsAsync(yearlyQuery()) { result, found ->
                val details = found.productDetailsList.firstOrNull()
                val offer = yearlyOffer(storeOffers(details))?.token
                    ?: details?.subscriptionOfferDetails?.firstOrNull()?.offerToken
                if (result.responseCode != BillingClient.BillingResponseCode.OK || details == null || offer == null) {
                    invoke.reject("not_on_sale")
                    return@queryProductDetailsAsync
                }
                buying = invoke
                val flow = BillingFlowParams.newBuilder()
                    .setProductDetailsParamsList(
                        listOf(
                            BillingFlowParams.ProductDetailsParams.newBuilder()
                                .setProductDetails(details)
                                .setOfferToken(offer)
                                .build(),
                        ),
                    )
                    .build()
                // launchBillingFlow has to run on the activity's thread
                activity.runOnUiThread {
                    if (client.launchBillingFlow(activity, flow).responseCode != BillingClient.BillingResponseCode.OK) {
                        buying = null
                        invoke.reject("store_unavailable")
                    }
                }
            }
        }
    }

    private fun queryEntitlement(answer: (Long?) -> Unit) {
        withBilling({ answer(null) }) { client ->
            val subs = QueryPurchasesParams.newBuilder()
                .setProductType(BillingClient.ProductType.SUBS)
                .build()
            client.queryPurchasesAsync(subs) { result, purchases ->
                if (result.responseCode != BillingClient.BillingResponseCode.OK) {
                    answer(null)
                    return@queryPurchasesAsync
                }
                acknowledge(client, purchases)
                answer(activeUntil(storePurchases(purchases), System.currentTimeMillis()))
            }
        }
    }

    @Command
    fun subscription(invoke: Invoke) {
        queryEntitlement { until ->
            if (until == null) {
                invoke.reject("store_unavailable")
            } else {
                invoke.resolve(JSObject().apply { put("until", until) })
            }
        }
    }

    private fun tellEntitlement() {
        queryEntitlement { until -> until?.let { EntitlementEvents.offer(it) } }
    }

    @Command
    fun registerEntitlementEvents(invoke: Invoke) {
        try {
            EntitlementEvents.register(invoke.parseArgs(CallEventsArgs::class.java).channel)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "no channel")
        }
    }

    @Command
    fun restoreSubscription(invoke: Invoke) = subscription(invoke)

    @Command
    fun saveToDownloads(invoke: Invoke) {
        if (!canSaveToDownloads(Build.VERSION.SDK_INT)) {
            invoke.reject("saving to Downloads needs Android 10")
            return
        }
        try {
            val args = invoke.parseArgs(SaveFileArgs::class.java)
            val resolver = activity.contentResolver
            val details = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, args.name)
                put(MediaStore.Downloads.MIME_TYPE, args.mime)
                put(MediaStore.Downloads.IS_PENDING, 1)
            }
            val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, details)
                ?: throw IllegalStateException("Downloads is not available")
            resolver.openOutputStream(uri).use { output ->
                File(args.path).inputStream().use { input -> input.copyTo(output!!) }
            }
            resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot save the file")
        }
    }

    // the file has to stay until the print service has read it
    @Command
    fun printFile(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SaveFileArgs::class.java)
            val file = File(args.path)
            if (!file.exists()) throw IllegalStateException("there is nothing to print")
            activity.runOnUiThread {
                if (args.mime.startsWith("image/")) {
                    PrintHelper(activity).apply { scaleMode = PrintHelper.SCALE_MODE_FIT }
                        .printBitmap(args.name, BitmapFactory.decodeFile(file.path))
                } else {
                    val manager = activity.getSystemService(Context.PRINT_SERVICE) as PrintManager
                    manager.print(args.name, FileToPrint(args.name, file), null)
                }
            }
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot print the file")
        }
    }
}

private class FileToPrint(private val name: String, private val file: File) : PrintDocumentAdapter() {
    override fun onLayout(
        old: PrintAttributes?,
        new: PrintAttributes?,
        signal: CancellationSignal?,
        callback: LayoutResultCallback,
        extras: Bundle?,
    ) {
        if (signal?.isCanceled == true) {
            callback.onLayoutCancelled()
            return
        }
        val info = PrintDocumentInfo.Builder(name)
            .setContentType(PrintDocumentInfo.CONTENT_TYPE_DOCUMENT)
            .setPageCount(PrintDocumentInfo.PAGE_COUNT_UNKNOWN)
            .build()
        callback.onLayoutFinished(info, true)
    }

    override fun onWrite(
        pages: Array<out PageRange>?,
        destination: ParcelFileDescriptor,
        signal: CancellationSignal?,
        callback: WriteResultCallback,
    ) {
        try {
            file.inputStream().use { input ->
                java.io.FileOutputStream(destination.fileDescriptor).use { output -> input.copyTo(output) }
            }
            callback.onWriteFinished(arrayOf(PageRange.ALL_PAGES))
        } catch (error: Exception) {
            callback.onWriteFailed(error.message)
        }
    }
}
