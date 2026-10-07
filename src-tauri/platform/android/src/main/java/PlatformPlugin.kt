package com.flickertalk.platform

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.app.Notification
import android.app.Service
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.view.WindowManager
import android.os.IBinder
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

/**
 * The plugin's own FileProvider class: the app already declares `androidx.core.content.FileProvider`
 * (wry's file chooser), and two providers of one class cannot be merged into the manifest.
 */
class FtFileProvider : FileProvider()

/** The most pictures the photo picker takes at once. */
const val PICK_LIMIT = 20

/** The FileProvider declared in this plugin's manifest. */
fun fileProviderAuthority(packageName: String): String = "$packageName.ft.files"

fun canSaveToDownloads(sdk: Int): Boolean = sdk >= Build.VERSION_CODES.Q

/**
 * Whether a request for pictures can use the photo picker: a sheet that opens over the app, closes
 * with a swipe and asks for no permission. Android 13 and up (§62).
 */
fun usesPhotoPicker(accept: String, sdk: Int): Boolean =
    (accept.startsWith("image/") || accept.startsWith("video/")) && sdk >= Build.VERSION_CODES.TIRAMISU

/** What the photo picker is told to show: pictures, videos, or (with no type) both. */
fun photoPickerType(accept: String): String? =
    accept.takeIf { it == "image/*" || it == "video/*" || Regex("^(image|video)/[a-z0-9.+-]+$").matches(it) }

/** What the system's document picker is told to show. Anything we do not understand means all. */
fun documentType(accept: String): String =
    if (Regex("^[a-z]+/([a-z0-9.+-]+|\\*)$").matches(accept)) accept else "*/*"

/** The router's push only ever says "wake" (Plan §12): no sender, no content. */
fun isWake(data: Map<String, String>): Boolean = data["t"] == "wake"

/**
 * Whether a wake-up should be heard (app#9). `slot` says which of the device's eight route
 * capabilities the sender used: 0 (or none, from an older router) is the device's own; 1–7 belong
 * to hidden sessions, heard only while open. A process a push starts reads the open ones from
 * preferences (`openSlotsNow`): a session stays open until the user leaves it (2026-10-01).
 */
fun wakeIsHeard(slot: String?, open: Set<Int>): Boolean {
    if (slot == null || slot == "0") return true
    return slot.toIntOrNull()?.let { it in open } ?: false
}

/** The open slots as preferences keep them. */
fun keptSlots(slots: Set<Int>): String = slots.sorted().joinToString(",")

/** The open slots read back from preferences: only 1–7; nothing kept is none open. */
fun slotsKept(kept: String?): Set<Int> =
    kept.orEmpty().split(",").mapNotNull { it.trim().toIntOrNull() }.filter { it in 1..7 }.toSet()

private const val OPEN_SLOTS = "open_slots"

/**
 * The hidden sessions open now, by slot, as the core last said (2026-10-01): kept in preferences,
 * so a process a push starts before the core knows them too. Never which session, only the slot.
 */
fun openSlotsNow(context: Context): Set<Int> =
    slotsKept(context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString(OPEN_SLOTS, null))

/** Written at once: a push may start a new process right after. */
fun keepOpenSlots(context: Context, slots: Set<Int>) {
    context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().putString(OPEN_SLOTS, keptSlots(slots)).commit()
}

/**
 * Whether to show the system prompt for notifications: Android 13 and up, while not granted, and
 * at most once per process start (app#70). Closing the prompt makes the page visible again, which
 * hands the push token over once more; that must not ask again, nor must every return to the
 * screen after a refusal. Process-wide, so a recreated activity does not ask again either.
 */
class NotificationPrompt {
    private val asked = AtomicBoolean(false)

    fun shouldAsk(sdk: Int, granted: Boolean): Boolean = sdk >= 33 && !granted && asked.compareAndSet(false, true)

    companion object {
        val process = NotificationPrompt()
    }
}

/** An app on screen is already connected and gets everything: no notification then. */
fun shouldNotify(importance: Int): Boolean =
    importance > ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND

private const val CHANNEL = "ft.activity"
private const val NOTIFICATION = 1

/** Calls ring on their own channel, so messages and calls can be set apart (§66). */
const val CALL_CHANNEL = "ft.call"
const val CALL_NOTIFICATION = 2

/** Who is calling; null for a contact with no name, who is still a caller ("Someone"). */
fun callTitle(name: String): String? = name.trim().ifEmpty { null }

fun callText(video: Boolean): Int = if (video) R.string.ft_incoming_video_call else R.string.ft_incoming_call

/**
 * FCM wake-ups (M4). The push carries nothing to read: when the app is not on screen, a plain
 * notification invites the user to open it; opening it connects and fetches what waits.
 */
class FtMessagingService : FirebaseMessagingService() {
    override fun onMessageReceived(message: RemoteMessage) {
        // A live core reconnects now: its socket to the router is dead (2026-09-28).
        val openSlots = openSlotsNow(this)
        if (reconnectsOnPush(message.data, openSlots)) CallEvents.offer(CallEvent.Incoming)
        if (isCall(message.data)) {
            incomingCall(message, openSlots)
            return
        }
        if (!isWake(message.data)) return
        // A closed hidden session makes no noise, not even this (app#9).
        if (!wakeIsHeard(message.data["s"], openSlots)) return
        val state = ActivityManager.RunningAppProcessInfo()
        ActivityManager.getMyMemoryState(state)
        // Outside the weekly hours (app#7) the message still arrives when the app opens.
        if (shouldNotify(state.importance) && mayDisturbNow(this)) showActivityNotification(this)
    }

    /**
     * A call with the app closed (native calls, 2026-09-28): the incoming-call notification, with
     * the generic text, ringing for what is left of its 45 s. The system plays the ringtone on the
     * ringing channel, as the ringer mode says. Since 2026-10-01 the push also starts the core
     * (`PushCore`), with no activity: it connects at once, the notification learns who calls, and
     * the caller's hang-up, a refusal or a missed call end it as with the app open. A foreground
     * service of type phone call keeps the process awake meanwhile: one FCM woke is frozen within
     * seconds otherwise.
     */
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

    // A new token reaches the router the next time the app starts.
    override fun onNewToken(token: String) {}
}

// ---- Native calls (2026-09-28) ----

/** The router's call push (`{"t":"call","s":"N"}`): ring with the app closed (§66). */
fun isCall(data: Map<String, String>): Boolean = data["t"] == "call"

/** What a call push does. */
enum class CallPush { IGNORE, RING, SILENT }

/** Whether a call push rings, shows quietly or is dropped. */
fun callPush(data: Map<String, String>, open: Set<Int>, appOnScreen: Boolean, mayDisturb: Boolean): CallPush = when {
    !isCall(data) || !wakeIsHeard(data["s"], open) -> CallPush.IGNORE
    // The app on the screen is connected: the core rings the call itself (`startRinging`).
    appOnScreen -> CallPush.IGNORE
    !mayDisturb -> CallPush.SILENT
    else -> CallPush.RING
}

/**
 * Whether the core's refusal of a call without a trace (2026-10-01: Calls off, a stranger, a
 * blocked contact; §109) ends the call notification: only the one a call push started before the
 * core knew who called. A call the core rings itself (`startRinging`) or one going on is another.
 */
fun refusalCancels(coreRinging: Boolean, inCall: Boolean): Boolean = !coreRinging && !inCall

/** How long a call rings, in ms, at most. The router gives the push the same 45 s to live. */
const val CALL_RING_MS = 45_000L

/** How long a call pushed at `sentAt` still rings at `now`. */
fun callRingMillis(sentAt: Long, now: Long): Long {
    val elapsed = now - sentAt
    return if (sentAt > 0 && elapsed in 0 until CALL_RING_MS) CALL_RING_MS - elapsed else CALL_RING_MS
}

/** What the user does with a native call, as Rust's `NativeCallEvent`. */
sealed class CallEvent {
    /** A call or wake-up push: the router found this phone offline; the core reconnects now. */
    object Incoming : CallEvent()
    object Answer : CallEvent()
    object End : CallEvent()
    /** The incoming call notification's decline: before the offer came, the core waits for it. */
    object Decline : CallEvent()
    data class Mute(val muted: Boolean) : CallEvent()
    object AudioActivated : CallEvent()
    object AudioDeactivated : CallEvent()
    /** The app came to the screen or left it (native video): the core holds our camera meanwhile. */
    data class Visible(val visible: Boolean) : CallEvent()
    /** The display turned, in degrees, while the call has its video views. */
    data class Orientation(val degrees: Int) : CallEvent()
    /** The ongoing call notification's camera action: the core turns our camera on. */
    object VideoRequested : CallEvent()
}

/** The event as the channel carries it: `{"event": "mute", "muted": true}`. */
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
}

/**
 * Events wait here until the core listens, then go out in order. Before the app, a core a call
 * push started (2026-10-01, `PushCore`) takes what `pushCoreTakes` says; the rest (an answer)
 * waits for the app, whose core then takes everything.
 */
class CallEventQueue(private val limit: Int = 16) {
    private var sink: ((CallEvent) -> Unit)? = null
    /** A core a call push started, before the app: `false` when it could not take the event. */
    private var pushCore: ((CallEvent) -> Boolean)? = null
    private val waiting = ArrayDeque<CallEvent>()

    /** The core listens: what waited goes out now, in order; a previous listener hears no more. */
    @Synchronized
    fun register(sink: (CallEvent) -> Unit) {
        this.sink = sink
        pushCore = null
        while (waiting.isNotEmpty()) sink(waiting.removeFirst())
    }

    /** A core a call push started is up: what it takes of what waited goes to it now. Never
     *  over the app's core. */
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

    /** The push's core stopped: events wait for the app again. */
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

    /** Whether the app's core listens now (it runs with the app: Tauri starts Rust with it). */
    @Synchronized
    fun listening(): Boolean = sink != null

    /** Whether a core hears `event` now: the app's, or the push's for what it takes. */
    @Synchronized
    fun hears(event: CallEvent): Boolean = sink != null || (pushCore != null && pushCoreTakes(event))

    /** A new call starts: what an old one left unheard is no longer true. */
    @Synchronized
    fun forget() = waiting.clear()

    /** Only to a core that listens now; nothing waits (a process FCM started has no core yet). */
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

/**
 * Whether a push tells the core to reconnect at once (2026-09-28): a call or a wake-up means the
 * router found this phone offline, so its socket is dead. A closed hidden session stays quiet.
 */
fun reconnectsOnPush(data: Map<String, String>, open: Set<Int>): Boolean =
    (isCall(data) || isWake(data)) && wakeIsHeard(data["s"], open)

/** How the call's voice goes to the speaker or back to the earpiece. */
enum class SpeakerRoute { SPEAKER_DEVICE, CLEAR_DEVICE, SPEAKERPHONE_ON, SPEAKERPHONE_OFF }

/** Android 12 picks the communication device; before, the speakerphone switch. */
fun speakerRoute(sdk: Int, on: Boolean): SpeakerRoute = when {
    sdk >= Build.VERSION_CODES.S -> if (on) SpeakerRoute.SPEAKER_DEVICE else SpeakerRoute.CLEAR_DEVICE
    else -> if (on) SpeakerRoute.SPEAKERPHONE_ON else SpeakerRoute.SPEAKERPHONE_OFF
}

/** The app shows over the lock screen, and turns the screen on, while a call rings or goes on. */
fun overLockScreen(ringing: Boolean, inCall: Boolean): Boolean = ringing || inCall

/**
 * Whether the call's service may take the microphone type (Android 14): only with the app on the
 * screen or right after the user's own answer; otherwise the system refuses the whole service.
 */
fun microphoneServiceAllowed(granted: Boolean, visible: Boolean, answeredByTap: Boolean): Boolean =
    granted && (visible || answeredByTap)

/** Shows the app over the lock screen and turns the screen on, or stops doing so. */
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

/** The audio mode to go back to after a call: what it was, unless that was a call's mode too. */
fun modeAfterCall(previous: Int?): Int = when (previous) {
    null, AudioManager.MODE_IN_COMMUNICATION, AudioManager.MODE_IN_CALL -> AudioManager.MODE_NORMAL
    else -> previous
}

/** A call pushed with the app closed rings on this channel: the system plays the ringtone. */
const val RINGING_CALL_CHANNEL = "ft.call.ringing"

/**
 * The foreground service types of a call: phone call, the microphone once it is allowed, and the
 * camera while the call has video (native video, 2026-09-29). Types beyond phone call exist from
 * Android 11.
 */
// The types are compile-time constants, used only past the `sdk` checks lint cannot follow.
@SuppressLint("InlinedApi")
fun callServiceTypes(sdk: Int, microphone: Boolean, camera: Boolean = false): Int {
    if (sdk < Build.VERSION_CODES.Q) return 0
    var types = ServiceInfo.FOREGROUND_SERVICE_TYPE_PHONE_CALL
    if (sdk < Build.VERSION_CODES.R) return types
    if (microphone) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
    if (camera) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA
    return types
}

/**
 * The incoming call on the screen: its own high channel, the call category and a full-screen
 * intent that opens the app over the lock screen. If the system does not allow full screen (§66,
 * Android 14 keeps it for calling apps), it still shows as a heads-up notification.
 */
/** Opening the app, and nothing more: the buttons go through `tapIntent` (NotificationTaps.kt). */
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

/** How the system plays the ringtone of a call pushed with the app closed. A function, not a
 *  value: a top-level value would run Android code when the JVM tests load this file. */
private fun ringtoneAttributes(): AudioAttributes = AudioAttributes.Builder()
    .setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE)
    .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
    .build()

/**
 * `ringing`: the system rings (insistently, as the ringer mode says) until the notification goes,
 * after `timeoutMs` at most. Otherwise the notification is silent: the app rings itself, or the
 * weekly hours keep the call quiet.
 */
fun showCall(context: Context, title: String, text: String, ringing: Boolean = false, timeoutMs: Long = 0, video: Boolean = false) {
    callNotification(context, title, text, ringing, timeoutMs, video)?.let { IncomingCall.post(context, it) }
}

/**
 * The incoming call notification (`showCall`). `alertOnce`: a quiet update of one already shown
 * (the push's call learns its caller is muted, 2026-10-01). A ringing one is never updated: Android
 * then stops the insistent ringtone (`pushRing`).
 */
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
                setSound(null, null) // the ringtone is ours, so the notification stays quiet
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
    // The buttons hand the tap to the process at once, whatever state the app is in (2026-09-29).
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
        // Shown at once even as a foreground service's notification (the push's, 2026-10-01).
        .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
        .setContentIntent(open)
        .setFullScreenIntent(open, true)
        // Answer and decline from the notification itself: the user should not have to open the
        // app to pick up (§66).
        .setStyle(NotificationCompat.CallStyle.forIncomingCall(caller, decline, answer).setIsVideo(video))
    if (timeoutMs > 0) builder.setTimeoutAfter(timeoutMs)
    if (ringing) {
        // Before Android 8 the sound and vibration are the notification's, not its channel's.
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
        // Notifications not allowed: the app gets everything when it opens.
    }
}

// ---- A native call in progress (2026-09-28) ----

/** The ongoing call's notification: the foreground service's, so it has its own id. */
private const val ONGOING_CALL_NOTIFICATION = 3
/** The ongoing call shows on a quiet channel: it is there to hang up and mute, not to alert. */
const val ONGOING_CALL_CHANNEL = "ft.call.ongoing"
private const val ACTION_HANG_UP = "com.flickertalk.platform.HANG_UP"
private const val ACTION_MUTE = "com.flickertalk.platform.MUTE"

/**
 * The core's channel (`listen_calls`) and the events that wait for it. One background thread
 * sends them, so Rust's handler never runs on the main thread and may call the plugin back.
 */
object CallEvents {
    private val queue = CallEventQueue()
    private val sender = java.util.concurrent.Executors.newSingleThreadExecutor()

    fun register(channel: Channel) = sender.execute {
        queue.register { channel.sendObject(callEventPayload(it)) }
    }

    fun emit(event: CallEvent) = sender.execute { queue.emit(event) }

    fun offer(event: CallEvent) = sender.execute { queue.offer(event) }

    fun forget() = sender.execute { queue.forget() }

    /** A core a call push started is up (2026-10-01): it takes a decline, a hang-up, the push. */
    fun registerPushCore(take: (CallEvent) -> Boolean) = sender.execute { queue.registerPushCore(take) }

    fun forgetPushCore() = sender.execute { queue.forgetPushCore() }

    /** Whether the core listens now; a registration still on its way counts as not yet. */
    fun listening(): Boolean = queue.listening()

    /** Whether a core hears `event` now (the app's, or the push's for what it takes). */
    fun hears(event: CallEvent): Boolean = queue.hears(event)
}

/**
 * The ringtone and vibration of a call the core rings with the app running (`startRinging`): the
 * process's, so a tap on the notification's buttons silences them before the plugin hears of it.
 */
object CallRinger {
    var ringtone: Ringtone? = null
    var vibrator: Vibrator? = null
    /** The core rings a call it knows (`startRinging`), until `stopRinging` or an answer. */
    @Volatile
    var coreRinging = false

    fun silence() {
        ringtone?.stop()
        ringtone = null
        vibrator?.cancel()
        vibrator = null
    }
}

/**
 * The call this phone is in (native calls): communication audio mode, audio focus for voice,
 * and a foreground service of type phone call and microphone, so the microphone keeps working
 * with the app in the background (Android 14). Used on the main thread.
 */
object InCall {
    var active = false
        private set
    var muted = false
    var name = ""
        private set
    /** Who the last ringing call was: an answered call is named after it. */
    var ringingName = ""
    /** The app is on the screen (the plugin's activity is resumed). */
    @Volatile
    var appVisible = false
    /** The user answered this call with their own tap (the notification's button, or the app's). */
    @Volatile
    var answeredByTap = false
    /** The call has video now, either camera on (`callVideo`, native video 2026-09-29). */
    @Volatile
    var video = false
    /** The user asked for video with their own tap on the ongoing notification's camera action. */
    @Volatile
    var videoByTap = false
    private var previousMode: Int? = null
    private var focus: Any? = null
    @Volatile
    var service: FtCallService? = null

    /** Starts the call's audio and service; again, only makes sure the service holds what it may. */
    fun start(context: Context, name: String) {
        if (!active) {
            this.name = name
            active = true
            muted = false
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
            // Android 12+ refuses a foreground service started from the background; the call
            // still works while the app is on the screen.
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
        speaker(audio, false)
        audio.mode = modeAfterCall(previousMode)
        previousMode = null
    }

    /** The call's voice on the speaker, or back on the earpiece (2026-09-28). */
    fun setSpeaker(context: Context, on: Boolean) {
        val audio = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
        speaker(audio, on)
    }

    private fun speaker(audio: AudioManager, on: Boolean) {
        when (speakerRoute(Build.VERSION.SDK_INT, on)) {
            SpeakerRoute.SPEAKER_DEVICE -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                audio.availableCommunicationDevices.firstOrNull { it.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER }?.let { audio.setCommunicationDevice(it) }
            }
            SpeakerRoute.CLEAR_DEVICE -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) audio.clearCommunicationDevice()
            SpeakerRoute.SPEAKERPHONE_ON, SpeakerRoute.SPEAKERPHONE_OFF -> {
                @Suppress("DEPRECATION")
                audio.isSpeakerphoneOn = on
            }
        }
    }
}

/** The ongoing call's notification: who, hang up and mute; tapping it opens the app. */
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
        // Published again on each change: CallStyle has no live switch between voice and video.
        .setStyle(NotificationCompat.CallStyle.forOngoingCall(caller, action(ACTION_HANG_UP, 5)).setIsVideo(InCall.video))
        .addAction(
            R.drawable.ft_notification,
            context.getString(if (InCall.muted) R.string.ft_unmute else R.string.ft_mute),
            action(ACTION_MUTE, 6),
        )
    // The camera (native video, 2026-09-29): opens the app, where the core turns our camera on;
    // the camera only runs with the app on the screen.
    builder.addAction(R.drawable.ft_notification, context.getString(R.string.ft_video), tapIntent(context, NotificationTap.CALL_VIDEO))
    openAppIntent(context, 4)?.let { builder.setContentIntent(it) }
    return builder.build()
}

/** Keeps the call alive with the app in the background: the microphone and the audio go on. */
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

    /**
     * In the foreground with the types it may hold now: the microphone once it is allowed, the
     * camera while the call has video (native video, 2026-09-29). Called again on each change:
     * `startForeground` with the whole set of types.
     */
    fun promote() {
        val recording = ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        val filming = ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
        val microphone = microphoneServiceAllowed(recording, InCall.appVisible, InCall.answeredByTap)
        val camera = cameraServiceAllowed(InCall.video, filming, InCall.appVisible, InCall.videoByTap)
        val sdk = Build.VERSION.SDK_INT
        // A type the system refuses now (from the background, or no leave for a phone-call
        // service) fails the whole call: try with less, the camera first, and give up if even the
        // phone call alone is refused.
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

    /** The notification again, after a change (mute). */
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

/** Hang up and mute on the ongoing call's notification: to the core, through its channel. */
class FtCallActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            ACTION_HANG_UP -> {
                CallEvents.emit(CallEvent.End)
                // The notification goes at once; the core's `callEnded` then finds it done.
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

// ---- Local reminders (2026-09-27): a plugin's alarm, shown by this phone alone ----

/** The channel reminders go to, apart from messages and calls, so the user can silence it alone. */
const val REMINDER_CHANNEL = "ft.reminders"
/** Where the reminder list is kept for the boot receiver: the core is the truth, this is a copy. */
private const val REMINDERS = "reminders"
/** The extra that carries the URL a login sent the user back with. */
const val AUTH_RESULT = "ft.auth.result"
/** Notification ids for reminders start here; messages and calls use the first few. */
private const val REMINDER_NOTIFICATION_BASE = 1000

/** One reminder as the core wrote it. */
data class ReminderEntry(val plugin: String, val id: String, val at: Long, val text: String)

/** What the core sends: `[{plugin, id, at, text}]`. Anything malformed is skipped, not fatal. */
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

/** A stable request code per reminder, so the same reminder replaces its own alarm. */
fun reminderRequestCode(plugin: String, id: String): Int =
    REMINDER_NOTIFICATION_BASE + ((plugin + "\n" + id).hashCode() and 0x7fffffff) % 1_000_000

/** `plugin\nid`, what the app is opened with when a reminder is tapped. */
fun reminderKey(plugin: String, id: String): String = plugin + "\n" + id

/** The reminder key an intent carries, or nothing. */
fun pendingReminderOf(value: String?): String = value?.takeIf { it.contains('\n') } ?: ""

/** Whether exact alarms are allowed on this phone (Android 12 asks; 14 denies by default). */
fun exactAlarmsAllowed(sdk: Int, canSchedule: () -> Boolean): Boolean =
    sdk < Build.VERSION_CODES.S || canSchedule()

/** Whether a login's redirect is the one we wait for: our scheme, not some other link. */
fun isAuthRedirect(uri: String?, scheme: String): Boolean =
    uri != null && scheme.isNotEmpty() && uri.startsWith("$scheme:")

/** Reminders whose time has not passed: the boot receiver schedules only these. */
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

/** Cancels every alarm of the copy kept in preferences, then schedules `entries` and keeps them. */
fun scheduleReminders(context: Context, entries: List<ReminderEntry>) {
    val alarms = context.getSystemService(AlarmManager::class.java) ?: return
    val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
    for (old in parseReminders(preferences.getString(REMINDERS, "[]") ?: "[]")) {
        alarms.cancel(reminderIntent(context, old))
    }
    val exact = exactAlarmsAllowed(Build.VERSION.SDK_INT) {
        Build.VERSION.SDK_INT < Build.VERSION_CODES.S || alarms.canScheduleExactAlarms()
    }
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

/** The alarm rang: a notification that opens the app on that reminder. Says nothing of the
 *  note unless the core sent a text (the user allowed content on the lock screen). */
class ReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val plugin = intent.getStringExtra("plugin") ?: return
        val id = intent.getStringExtra("id") ?: return
        showReminder(context, plugin, id, intent.getStringExtra("text").orEmpty())
    }
}

/** A reminder's notification; its tap hands the reminder to the process, then opens the app. */
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
        // Notifications not allowed: the plugin shows the reminder as due when the app opens.
    }
}

/** Alarms do not survive a reboot: they are set again from the copy in preferences. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        val kept = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString(REMINDERS, "[]") ?: "[]"
        scheduleReminders(context, parseReminders(kept))
    }
}

/** The provider sent the user back after a login (drive, 2026-09-27): hand the URL to the app. */
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

/** A sealed key is the GCM nonce followed by the ciphertext and its tag. */
fun splitSealed(sealed: ByteArray): Pair<ByteArray, ByteArray>? =
    if (sealed.size <= IV_BYTES) null else Pair(sealed.copyOfRange(0, IV_BYTES), sealed.copyOfRange(IV_BYTES, sealed.size))

/** The Keystore's AES key for the storage key: made once, never leaves the Keystore (§94). */
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

/** Seals the storage key with the Keystore's AES key (§94): the nonce, then the ciphertext. */
fun sealStorageKey(key: ByteArray): ByteArray {
    val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, keystoreKey()) }
    return cipher.iv + cipher.doFinal(key)
}

/** Opens a sealed storage key. No activity needed: a core a call push started opens it too. */
fun openStorageKey(sealed: ByteArray): ByteArray {
    val (iv, ciphertext) = splitSealed(sealed) ?: throw IllegalArgumentException("not a sealed key")
    val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply {
        init(Cipher.DECRYPT_MODE, keystoreKey(), GCMParameterSpec(128, iv))
    }
    return cipher.doFinal(ciphertext)
}

/** How an incoming call rings. */
data class Ringing(val sound: Boolean, val vibrate: Boolean)

/**
 * As the user set the phone: sound and vibration, vibration only, or nothing. A quiet call (a
 * muted contact, app#4, or outside the weekly hours, app#7) shows but makes no noise.
 */
fun ringingFor(ringerMode: Int, quiet: Boolean = false): Ringing = when {
    quiet -> Ringing(sound = false, vibrate = false)
    ringerMode == AudioManager.RINGER_MODE_NORMAL -> Ringing(sound = true, vibrate = true)
    ringerMode == AudioManager.RINGER_MODE_VIBRATE -> Ringing(sound = false, vibrate = true)
    else -> Ringing(sound = false, vibrate = false)
}

/**
 * Whether the phone may make noise now under the weekly hours (app#7). `week` is what the core
 * hands over: seven days, Monday first, separated by `;`, each `all`, `none` or `FROM-TO` in
 * minutes of the day (past midnight when TO comes first). Empty means no hours: always.
 */
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

/** The weekly hours as the core last handed them over, checked against the phone's clock. */
fun mayDisturbNow(context: Context): Boolean {
    val week = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString(QUIET_HOURS, "") ?: ""
    val now = java.util.Calendar.getInstance()
    val monday = (now.get(java.util.Calendar.DAY_OF_WEEK) + 5) % 7
    return mayDisturb(week, monday, now.get(java.util.Calendar.HOUR_OF_DAY) * 60 + now.get(java.util.Calendar.MINUTE))
}

/** Ring 0.8 s, pause 1.2 s, and again. */
private val RING_PATTERN = longArrayOf(0, 800, 1200)

@InvokeArg
class RingingArgs {
    var caller: String = ""
    var video: Boolean = false
    /** A muted contact (app#4): the call shows but makes no noise. */
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

/** Where a picked file is copied, inside the app's own folder. */
const val PICKED_FOLDER = "uploads"

/** A photo taken with the camera is named by when it was taken, like a voice note. */
fun photoName(now: Long, zone: java.util.TimeZone = java.util.TimeZone.getDefault()): String {
    val format = java.text.SimpleDateFormat("'photo-'yyyyMMdd-HHmmss'.jpg'", java.util.Locale.ROOT)
    format.timeZone = zone
    return format.format(java.util.Date(now))
}

/** How one picked file's copy is named, as on iOS: when it was picked, its place in the pick and
 *  its name, which never makes a path. */
fun pickedBase(now: Long, index: Int, name: String): String = "$now-$index-${name.replace('/', '_')}"

/**
 * A new, empty file for a picked file or a photo, named `base` or, if that is taken (a clock set
 * back, two photos in one second), `base` with `-2`, `-3`… before its extension. Never one that is
 * there already: a message may point to it (2026-10-02). Created here, so nothing else gets it.
 */
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

/** A photo is kept only if the camera app came back with one that has bytes. */
fun keepsPhoto(resultCode: Int, size: Long): Boolean = resultCode == Activity.RESULT_OK && size > 0

/** The name to show for a picked file; something with no name is still a file. */
fun pickedName(name: String?): String = name?.trim()?.takeIf { it.isNotEmpty() } ?: "file"

/** What a picked file is, as far as we can tell. */
fun pickedMime(mime: String?): String = mime?.trim()?.takeIf { it.isNotEmpty() } ?: "application/octet-stream"

/** The text for the share sheet, or null when there is nothing to share. */
fun shareableText(text: String): String? = text.trim().ifEmpty { null }

/** androidx's dark scrim, behind light navigation icons that cannot turn dark (before Android 8). */
val NAVIGATION_SCRIM: Int = 0x801B1B1B.toInt()

/**
 * The WebView text zoom (percent) for a font scale (app#92). A WebView takes it from the font
 * scale only when it is created, and the activity is no longer recreated for a new one.
 */
fun textZoomFor(fontScale: Float): Int = (fontScale * 100).roundToInt()

/** How the system bars look over the app: dark icons or light, and the navigation bar's colour. */
data class SystemBarsLook(val darkIcons: Boolean, val navigationBarColor: Int)

/**
 * The system bars for the app's appearance (2026-10-02): the app is dark by default whatever the
 * system's theme, so the icons follow the app. Both bars stay see-through (MainActivity), over the
 * app's header and the strip it paints under the navigation bar.
 */
fun systemBarsLook(dark: Boolean, sdk: Int): SystemBarsLook = SystemBarsLook(
    darkIcons = !dark,
    navigationBarColor = if (!dark && sdk < Build.VERSION_CODES.O) NAVIGATION_SCRIM else Color.TRANSPARENT,
)

/** The yearly subscription, as it is named in the Play Console (§40-42). */
const val YEARLY = "yearly"

/** The yearly subscription's base plan in the Play Console. */
const val YEARLY_BASE_PLAN = "yearly-autorenew"

/**
 * One offer of the subscription as Play describes it, with none of Google's classes in it: its
 * base plan, its own id (null for the base plan itself), the token a purchase names and the price
 * of each phase, formatted by the Store.
 */
data class StoreOffer(
    val basePlan: String,
    val offerId: String?,
    val token: String,
    val prices: List<String>,
)

/**
 * The offer the year is sold with: the yearly base plan itself, or else an offer of that plan;
 * null when Play lists neither (2026-09-29).
 */
fun yearlyOffer(offers: List<StoreOffer>): StoreOffer? {
    val yearly = offers.filter { it.basePlan == YEARLY_BASE_PLAN }
    return yearly.firstOrNull { it.offerId == null } ?: yearly.firstOrNull()
}

/**
 * What a year costs, exactly as the Store formats it for this phone, or null when it cannot say.
 * An offer's last phase is the price that keeps renewing, so an introductory price never shows.
 */
fun yearlyPrice(offers: List<StoreOffer>): String? =
    yearlyOffer(offers)?.prices?.lastOrNull()?.trim()?.takeIf { it.isNotEmpty() }

/** One purchase as the Store handed it back, with none of Google's classes in it. */
data class StorePurchase(
    val product: String,
    val state: Int,
    val boughtAt: Long,
    val acknowledged: Boolean,
)

/**
 * A year of FlickerTalk from the moment the Store took the money. The purchase only carries when
 * it was bought: asking Play's server for the expiry would mean our backend learning who pays
 * (§45-46), so the phone works it out and each renewal moves the date on.
 */
fun untilFromPurchase(boughtAt: Long): Long = boughtAt + 365L * 24 * 60 * 60 * 1000

/** Until when this phone is paid up, out of everything the Store returned; 0 when nothing is. */
fun activeUntil(purchases: List<StorePurchase>): Long =
    purchases
        .filter { it.product == YEARLY && it.state == Purchase.PurchaseState.PURCHASED }
        .maxOfOrNull { untilFromPurchase(it.boughtAt) } ?: 0L

/** What a purchase that pays for nothing yet says (2026-10-07); null when it pays. */
fun purchaseTrouble(purchases: List<StorePurchase>): String? =
    if (activeUntil(purchases) == 0L &&
        purchases.any { it.product == YEARLY && it.state == Purchase.PurchaseState.PENDING }
    ) {
        "pending_approval"
    } else {
        null
    }

/** Google gives the money back if a paid purchase is not acknowledged within three days. */
fun needsAcknowledgement(purchase: StorePurchase): Boolean =
    purchase.state == Purchase.PurchaseState.PURCHASED && !purchase.acknowledged

/**
 * What Play says about the subscription while the app runs (2026-10-07), on its way to the core:
 * until when the phone is paid up (ms). Before the core listens only the latest word waits: an
 * older one is no longer true.
 */
class EntitlementQueue {
    private var sink: ((Long) -> Unit)? = null
    private var latest: Long? = null

    /** The core listens: what waited goes out now; a previous listener hears no more. */
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

/** The channel Rust made (`listen_entitlements`) and the word that waits for it. */
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

/** What kind of file the app is asking the user for; empty means anything. */
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

/** What only Android lets Kotlin do (Plan §5): lend a file to a viewer, save it to Downloads. */
@TauriPlugin(
    permissions = [
        Permission(strings = [Manifest.permission.RECORD_AUDIO], alias = "microphone"),
        Permission(strings = [Manifest.permission.CAMERA], alias = "camera"),
        // The location plugin (2026-10-02): asking for both lets the user pick precise or approximate.
        Permission(strings = [Manifest.permission.ACCESS_COARSE_LOCATION, Manifest.permission.ACCESS_FINE_LOCATION], alias = "location"),
    ],
)
class PlatformPlugin(private val activity: Activity) : Plugin(activity) {
    /** A login waiting for the provider to send the user back, and the scheme it comes with. */
    private var authWaiting: Invoke? = null
    private var authScheme: String = ""

    /** The WebView the call's video views go under (native video). */
    private var webView: WebView? = null

    /** The app is open: the "something new" notification has done its job. */
    override fun load(webView: WebView) {
        super.load(webView)
        this.webView = webView
        InCall.appVisible = true
        activity.getSystemService(NotificationManager::class.java)?.cancel(NOTIFICATION)
        // Opened while a push's call rings (2026-10-01): the core a push started told the
        // notification, not this activity, so it shows over the lock screen from here.
        if (IncomingCall.showing) showOverLockScreen(activity, overLockScreen(ringing = true, inCall = InCall.active))
    }

    /** A new font size reaches the page in place: the activity is not recreated (app#92). */
    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        webView?.settings?.textZoom = textZoomFor(newConfig.fontScale)
    }

    /**
     * On the screen again: a call's service may now take the microphone (Android 14) and the
     * camera; the core lets our camera go on (native video).
     */
    override fun onResume() {
        super.onResume()
        InCall.appVisible = true
        CallEvents.offer(CallEvent.Visible(true))
        if (InCall.active) InCall.service?.promote()
        // What Play says now (2026-10-07): a renewal, an expiry, a refund, a purchase that was
        // pending, all while the app was away.
        tellEntitlement()
    }

    /**
     * Off the screen: the core holds our camera until the app is back (native video), and lets go
     * of the router and the other phones unless a call is going on (2026-10-01).
     */
    override fun onPause() {
        super.onPause()
        InCall.appVisible = false
        CallEvents.offer(CallEvent.Visible(false))
    }

    /**
     * The notifications' buttons (answer, decline, the camera, a reminder) no longer come here:
     * `FtNotificationTapActivity` hands them to the process (NotificationTaps.kt, 2026-09-29).
     */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // A login came back (drive): the command that opened it gets the URL.
        val result = intent.getStringExtra(AUTH_RESULT)
        intent.removeExtra(AUTH_RESULT)
        val waiting = authWaiting
        if (waiting != null && isAuthRedirect(result, authScheme)) {
            authWaiting = null
            waiting.resolve(JSObject().apply { put("url", result) })
        }
    }

    /** The reminder the user tapped to open the app, once (2026-09-27). */
    @Command
    fun pendingReminder(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("reminder", tappedReminder.take()) })
    }

    /** Every reminder there is, from the core: the alarm clock is set again from scratch. */
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

    /**
     * A login in the system's browser sheet (drive, 2026-09-27): Custom Tabs shows the provider's
     * page, the provider sends the user back to our scheme, `AuthRedirectActivity` brings the URL
     * here and the command resolves with it. The WebView sees none of it.
     */
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

    /**
     * The incoming call is being answered, or was before its offer came (2026-09-29): it rings no
     * more, the app stays over the lock screen, and the call is named after the caller. Its audio
     * and service start when it connects (`callConnected`).
     */
    @Command
    fun callAnswering(invoke: Invoke) {
        val args = invoke.parseArgs(AnsweringArgs::class.java)
        InCall.ringingName = args.caller
        CallRinger.coreRinging = false
        IncomingCall.dismiss(activity)
        silence()
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = true))
        invoke.resolve()
    }

    /** The core listens to native calls (`listen_calls`): events that waited go out now. */
    @Command
    fun registerCallEvents(invoke: Invoke) {
        try {
            CallEvents.register(invoke.parseArgs(CallEventsArgs::class.java).channel)
            // Whether the app is on the screen now: the core holds our camera while it is not.
            CallEvents.offer(CallEvent.Visible(InCall.appVisible))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "no channel")
        }
    }

    /** This phone calls: communication audio, focus and the call's foreground service. */
    @Command
    fun callStartedOutgoing(invoke: Invoke) {
        val args = invoke.parseArgs(OutgoingArgs::class.java)
        activity.runOnUiThread { InCall.start(activity, args.name) }
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = true))
        invoke.resolve()
    }

    /** Connected: a call answered in the app's own screen gets its audio and service here. */
    @Command
    fun callConnected(invoke: Invoke) {
        activity.runOnUiThread { InCall.start(activity, InCall.ringingName) }
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = true))
        invoke.resolve()
    }

    /** The call is over: the service stops, focus goes and the audio mode is what it was. */
    @Command
    fun callEnded(invoke: Invoke) {
        activity.runOnUiThread { InCall.end(activity) }
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = false))
        invoke.resolve()
    }

    /** The system bars' icons, as the app's appearance asks (2026-10-02); see `systemBarsLook`. */
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

    /** Speaker or earpiece for the call (2026-09-28). */
    @Command
    fun setSpeaker(invoke: Invoke) {
        val args = invoke.parseArgs(SpeakerArgs::class.java)
        activity.runOnUiThread { InCall.setSpeaker(activity, args.on) }
        invoke.resolve()
    }

    /**
     * The microphone before a native call (2026-09-28): the WebView used to ask through
     * `getUserMedia`. Asks only if it is not allowed yet; `granted` says what the user chose.
     */
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

    /**
     * The phone's current position, once (the location plugin, 2026-10-02): asks the first time
     * (precise or approximate is the user's choice), then one fix within ~15 s; `found` false when
     * refused, off or no fix. The core already checked that the plugin was granted `location`.
     */
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

    /**
     * The camera before our camera turns on (native video, 2026-09-29, §30): asks only if it is
     * not allowed yet; `granted` says what the user chose.
     */
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

    /**
     * The call's video views go behind the WebView, parked until `videoLayout` places them; their
     * surfaces go to Rust over JNI (`FtVideoSurfaces`). The layers of iOS are not used here.
     */
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

    /** Where the WebView leaves room for each picture, in CSS pixels; `null` hides one. */
    @Command
    fun videoLayout(invoke: Invoke) {
        val args = invoke.parseArgs(VideoLayoutArgs::class.java)
        activity.runOnUiThread {
            CallVideoViews.layout(args)
            invoke.resolve()
        }
    }

    /** The other side's picture, upright: its view takes that shape. */
    @Command
    fun videoShape(invoke: Invoke) {
        val args = invoke.parseArgs(VideoShapeArgs::class.java)
        activity.runOnUiThread {
            CallVideoViews.shape(args.width, args.height)
            invoke.resolve()
        }
    }

    /** The views go (their surfaces reach Rust as null first) and the WebView is opaque again. */
    @Command
    fun detachVideo(invoke: Invoke) {
        activity.runOnUiThread {
            CallVideoViews.detach()
            invoke.resolve()
        }
    }

    /**
     * Whether the call has video now (either camera on): the ongoing notification says so and the
     * call's service takes the camera type, or lets it go.
     */
    @Command
    fun callVideo(invoke: Invoke) {
        val on = invoke.parseArgs(CallVideoArgs::class.java).on
        activity.runOnUiThread {
            if (InCall.video != on) {
                InCall.video = on
                if (!on) InCall.videoByTap = false
                if (InCall.active) InCall.service?.promote()
            }
            invoke.resolve()
        }
    }

    /** Whether this phone lets us put a call on the whole screen (Android 14 and up). */
    @Command
    fun canShowFullScreen(invoke: Invoke) {
        val manager = activity.getSystemService(NotificationManager::class.java)
        val allowed = Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE ||
            manager?.canUseFullScreenIntent() == true
        invoke.resolve(JSObject().apply { put("allowed", allowed) })
    }

    /** Opens the system screen where the user allows calls to take the whole screen. */
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

    /**
     * An incoming call (§66): the user's ringtone and vibration, and a call notification with a
     * full-screen intent, so the call shows on the screen even with the app in the background or
     * the phone locked. It all goes away with `stopRinging`.
     */
    @Command
    fun startRinging(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(RingingArgs::class.java)
            InCall.ringingName = args.caller
            CallRinger.coreRinging = true
            showOverLockScreen(activity, overLockScreen(ringing = true, inCall = InCall.active))
            silence()
            showCall(
                activity,
                callTitle(args.caller) ?: activity.getString(R.string.ft_someone),
                activity.getString(callText(args.video)),
                video = args.video,
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

    /** Seals the storage key with the Keystore's AES key (§94). */
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

    /** Erasing the phone (2026-09-30): the Keystore key that sealed the storage key goes too. A
     *  new one is made the next time a key is sealed. */
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

    /** This device's FCM token, for the router to wake it (M4). */
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
            // No Firebase configuration in this build.
            invoke.reject(error.message ?: "push is not configured")
        }
    }

    /** Android 13 and up ask before showing notifications. */
    @Command
    fun requestNotifications(invoke: Invoke) {
        val granted = ContextCompat.checkSelfPermission(activity, "android.permission.POST_NOTIFICATIONS") == PackageManager.PERMISSION_GRANTED
        if (NotificationPrompt.process.shouldAsk(Build.VERSION.SDK_INT, granted)) {
            activity.requestPermissions(arrayOf("android.permission.POST_NOTIFICATIONS"), 4242)
        }
        invoke.resolve()
    }

    /** Starts the app again from its launcher activity, in a fresh process (§60). */
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

    /** Which hidden sessions are open, by slot (app#9): their wake-ups are heard, also by a
     *  process a push starts later (2026-10-01). */
    @Command
    fun setOpenSlots(invoke: Invoke) {
        keepOpenSlots(activity, invoke.parseArgs(OpenSlotsArgs::class.java).slots.toSet())
        invoke.resolve()
    }

    /** Keeps the weekly hours where the push service can read them with the app closed (app#7). */
    @Command
    fun setQuietHours(invoke: Invoke) {
        val week = invoke.parseArgs(QuietHoursArgs::class.java).week
        activity.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().putString(QUIET_HOURS, week).apply()
        invoke.resolve()
    }

    /**
     * The core refused an incoming call without a trace (2026-10-01): the call notification a
     * call push started for it, before the core knew who called, goes at once, and its ringing
     * with it. With the app closed, the core a call push started tells `PushCore.refused` instead.
     */
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
        IncomingCall.dismiss(activity)
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

    /** The system share sheet (WhatsApp, Signal, mail…) with a text, such as the card link (§32). */
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

    /**
     * Picks files with the system picker and copies them into the app's folder. The WebView's own
     * file input opens a screen the user cannot come back from without picking something; this one
     * returns to the app either way.
     */
    @Command
    fun pickFiles(invoke: Invoke) {
        val accept = try {
            invoke.parseArgs(PickArgs::class.java).accept
        } catch (_: Exception) {
            ""
        }
        val intent = if (usesPhotoPicker(accept, Build.VERSION.SDK_INT)) {
            // A sheet over the app: the user closes it and is still here, with nothing picked.
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

    /** The camera app takes a photo straight into the app's folder (Ioan, 2026-09-23). Without
     *  the camera permission (the app declares it for calls) the capture intent is refused, so
     *  it is asked for first and nothing is taken this time. */
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

    /** Copies what was picked into a new file of the app's folder, and says what it is. */
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

    /** Hands a file of the app to another app through the share sheet; the lending is read-only. */
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

    // What these commands reject with are keys, not sentences: the app turns them into the
    // user's own language (`plan.trouble.*`), so nothing raw ever reaches the screen.
    /** Play holds the money and the card; the app only ever asks and waits (§47). */
    private var billing: BillingClient? = null

    /** The call waiting for the Store window to close, since the answer arrives by listener. */
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

    /** What the Store handed back, without any of Google's classes going further in. */
    private fun storePurchases(purchases: List<Purchase>): List<StorePurchase> =
        purchases.flatMap { purchase ->
            purchase.products.map {
                StorePurchase(it, purchase.purchaseState, purchase.purchaseTime, purchase.isAcknowledged)
            }
        }

    /** Google refunds a purchase nobody acknowledged within three days, so it is told at once. */
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

    /**
     * How the Store answers a purchase: the window is gone and `subscribe` can be let go. With no
     * purchase waiting (a pending one that was paid, one made in the Play Store), it is still
     * acknowledged, and the core hears what the phone has now (2026-10-07).
     */
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
                // Waiting for the money is said as such; a purchase that pays for nothing else
                // answers 0, which the core calls a failed payment (2026-10-07).
                val trouble = purchaseTrouble(storePurchases(bought))
                if (trouble != null) {
                    invoke.reject(trouble)
                } else {
                    invoke.resolve(JSObject().apply { put("until", activeUntil(storePurchases(bought))) })
                }
            }
            BillingClient.BillingResponseCode.USER_CANCELED -> invoke.reject("cancelled")
            else -> invoke.reject("payment_failed")
        }
    }

    /** Play's question for the yearly subscription. */
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

    /** The subscription's offers, without any of Google's classes going further in. */
    private fun storeOffers(details: ProductDetails?): List<StoreOffer> =
        details?.subscriptionOfferDetails.orEmpty().map { offer ->
            StoreOffer(
                offer.basePlanId,
                offer.offerId,
                offer.offerToken,
                offer.pricingPhases.pricingPhaseList.map { it.formattedPrice },
            )
        }

    /**
     * What a year costs, as Play formats it for this phone (2026-09-29). Answers `{}` when the
     * Store cannot say (no product, no yearly plan): the screen then names no amount at all.
     */
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

    /**
     * The yearly subscription (§45, §47): asks Play for the product, opens its window and answers
     * until when the phone is paid up. Nothing about the payment ever reaches FlickerTalk.
     */
    @Command
    fun subscribe(invoke: Invoke) {
        withBilling(invoke) { client ->
            client.queryProductDetailsAsync(yearlyQuery()) { result, found ->
                val details = found.productDetailsList.firstOrNull()
                // The offer whose price the Plan screen shows; any other only if Play lists none.
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
                // The Store window belongs to the activity, so it is opened from its own thread.
                activity.runOnUiThread {
                    if (client.launchBillingFlow(activity, flow).responseCode != BillingClient.BillingResponseCode.OK) {
                        buying = null
                        invoke.reject("store_unavailable")
                    }
                }
            }
        }
    }

    /**
     * Asks Play for this account's subscriptions, acknowledges any that was not, and answers until
     * when the phone is paid up; null when Play does not answer.
     */
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
                answer(activeUntil(storePurchases(purchases)))
            }
        }
    }

    /** What the Store already knows about this phone, without asking anyone to buy anything. */
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

    /** The core hears what Play says now; nothing when Play does not answer (2026-10-07). */
    private fun tellEntitlement() {
        queryEntitlement { until -> until?.let { EntitlementEvents.offer(it) } }
    }

    /** The core listens to the Store's changes (`listen_entitlements`): what waited goes out now. */
    @Command
    fun registerEntitlementEvents(invoke: Invoke) {
        try {
            EntitlementEvents.register(invoke.parseArgs(CallEventsArgs::class.java).channel)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "no channel")
        }
    }

    /**
     * Restore purchases (2026-10-07): Play keeps the purchases of the Google account, so restoring
     * is asking for them once more, acknowledging any that was not, and keeping what they pay for.
     */
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

    /// Hands a file to the phone's print service. The user picks the printer; nothing is printed
    /// on its own, and the file stays in the app's folder until the service has read it (§53).
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

/// A file of the app handed to the print service as it is (a PDF, as the plugins make them).
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
