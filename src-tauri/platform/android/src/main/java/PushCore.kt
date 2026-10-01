package com.flickertalk.platform

import android.app.Notification
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Log
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import org.json.JSONObject

/*
 * A call push with the app closed (2026-10-01). The Rust library, and the core with it, used to
 * start only with the app's activity (Tauri loads it there): a process FCM started for a call had
 * no core, so the push's notification rang "Someone" until its own 45 s limit, the caller's
 * hang-up never arrived and no missed call was kept. Now the push loads the library and starts the
 * core with no activity and no WebView (`PushCore`, `src-tauri/src/push_core.rs`): it connects at
 * once, the notification learns who calls (or turns quiet, or goes, as the core decides), and the
 * call is kept as with the app open. When the app opens it takes this core; there is never a
 * second one. A foreground service of type phone call (`FtIncomingCallService`) keeps the process
 * awake while the push's call rings.
 */

/**
 * What a core a call push started takes before the app (Rust's `push_core_handles`): the push
 * itself (reconnect), the notification's decline and a hang-up. An answer waits for the app, which
 * the notification's answer opens (the microphone may need asking).
 */
fun pushCoreTakes(event: CallEvent): Boolean =
    event == CallEvent.Incoming || event == CallEvent.Decline || event == CallEvent.End

/** The event as the channel would carry it, as text for JNI (Rust's `parse_call_event`). */
fun callEventJson(event: CallEvent): String = JSONObject(callEventPayload(event)).toString()

/** How the push's call shows once the core knows who calls. */
enum class PushRing { NONE, RING, QUIET }

/**
 * A muted contact (app#4) or the weekly hours (app#7) make it quiet: no sound, no vibration. A
 * call the user answered from the notification already, or whose push has run out, is not shown.
 */
fun pushRing(muted: Boolean, mayDisturb: Boolean, answered: Boolean, leftMs: Long): PushRing = when {
    answered || leftMs <= 0 -> PushRing.NONE
    muted || !mayDisturb -> PushRing.QUIET
    else -> PushRing.RING
}

/** What is left of the push's ring time at `now`; nothing once it is over. */
fun ringLeft(deadline: Long, now: Long): Long = (deadline - now).coerceAtLeast(0)

/** Debug builds only, and only states: never who calls, never an id (§71). */
fun pushLog(context: Context, line: String) {
    if (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) Log.i("FtPushCore", line)
}

/**
 * The incoming call notification a call push shows (id `CALL_NOTIFICATION`) and the foreground
 * service that holds it while it rings. Every end of the call notification goes through `dismiss`:
 * a foreground service's notification cannot be cancelled while its service runs.
 */
object IncomingCall {
    @Volatile
    var notification: Notification? = null
        private set
    /** When the push's ring time ends (the phone's clock). */
    @Volatile
    private var deadline = 0L
    /** A call notification is up. */
    @Volatile
    var showing = false
        private set
    /** The user answered from the notification before the core knew who called. */
    @Volatile
    var answered = false
    /** Grows with each push: a late end of an older one leaves a newer one alone. */
    @Volatile
    private var generation = 0
    /** The service holding the notification, once in the foreground. Main thread. */
    private var service: FtIncomingCallService? = null
    private val main by lazy { Handler(Looper.getMainLooper()) }

    /** A call push: the notification rings, held by the foreground service if Android lets us. */
    fun ring(context: Context, notification: Notification, timeoutMs: Long) {
        this.notification = notification
        deadline = System.currentTimeMillis() + timeoutMs
        showing = true
        answered = false
        generation += 1
        try {
            ContextCompat.startForegroundService(context, Intent(context, FtIncomingCallService::class.java))
        } catch (_: Exception) {
            // Android 12+ may refuse it (FCM's allowance used up): it still rings, as before.
            post(context, notification)
        }
    }

    /** The notification again (the caller's name, or quiet): the service keeps holding it. */
    fun update(context: Context, notification: Notification) {
        this.notification = notification
        showing = true
        post(context, notification)
    }

    fun post(context: Context, notification: Notification) {
        try {
            context.getSystemService(NotificationManager::class.java)?.notify(CALL_NOTIFICATION, notification)
        } catch (_: SecurityException) {
            // Notifications not allowed: the core still keeps the call.
        }
    }

    /** The call notification goes, and its service with it. */
    fun dismiss(context: Context) {
        showing = false
        notification = null
        val ending = generation
        main.post { if (generation == ending) service?.end() }
        context.getSystemService(NotificationManager::class.java)?.cancel(CALL_NOTIFICATION)
    }

    fun left(now: Long): Long = ringLeft(deadline, now)

    /** The service is in the foreground: it goes at once if the call already went, or when the
     *  push's ring time ends. Main thread. */
    fun held(service: FtIncomingCallService) {
        this.service = service
        if (!showing) {
            service.end()
            return
        }
        val ringing = generation
        main.postDelayed({ if (generation == ringing) dismiss(service) }, left(System.currentTimeMillis()))
    }

    fun gone(service: FtIncomingCallService) {
        if (this.service === service) this.service = null
    }
}

/**
 * Keeps a process a call push started awake while its call rings: a process FCM woke is frozen
 * within seconds, its core with it. Type phone call (MANAGE_OWN_CALLS), started within the
 * allowance FCM gives a high-priority message. It holds the call notification itself.
 */
class FtIncomingCallService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Started for the foreground, it must get there even if the call went meanwhile.
        val notification = IncomingCall.notification
            ?: callNotification(this, getString(R.string.ft_someone), getString(R.string.ft_incoming_call))
        if (notification == null) {
            stopSelf()
            return START_NOT_STICKY
        }
        try {
            ServiceCompat.startForeground(this, CALL_NOTIFICATION, notification, callServiceTypes(Build.VERSION.SDK_INT, microphone = false))
        } catch (_: Exception) {
            IncomingCall.post(this, notification)
            stopSelf()
            return START_NOT_STICKY
        }
        IncomingCall.held(this)
        return START_NOT_STICKY
    }

    fun end() {
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        IncomingCall.gone(this)
        super.onDestroy()
    }
}

/**
 * The core a call push starts, with no activity (2026-10-01): Kotlin's side of
 * `src-tauri/src/push_core.rs`. `start` loads the app's library and hands the core the app's data
 * folder; Rust calls back the `@JvmStatic` functions below for the Keystore and the call screen.
 */
object PushCore {
    /** The app's Rust library (`[lib] name` in src-tauri/Cargo.toml). */
    const val LIBRARY = "flickertalk_lib"

    @Volatile
    private var context: Context? = null
    @Volatile
    private var loaded = false

    /** A call push came: the core starts, or (one runs already) hears that a call is on its way. */
    fun start(context: Context) {
        val app = context.applicationContext
        this.context = app
        try {
            if (!loaded) {
                System.loadLibrary(LIBRARY)
                loaded = true
            }
            pushLog(app, if (nativeStart(app.dataDir.absolutePath)) "core: starting" else "core: not started")
        } catch (_: Throwable) {
            pushLog(app, "core: no library")
        }
    }

    @JvmStatic
    private external fun nativeStart(dataDir: String): Boolean

    /** A notification button for the push's core; `false` if none listens (it waits for the app). */
    @JvmStatic
    external fun nativeCallEvent(json: String): Boolean

    // ---- Called by Rust (`push_core.rs`), on the core's threads ----

    @JvmStatic
    fun openKey(sealed: ByteArray): ByteArray = openStorageKey(sealed)

    @JvmStatic
    fun sealKey(key: ByteArray): ByteArray = sealStorageKey(key)

    /** The core is up: the notification's decline (and the next push) reach it from now on. */
    @JvmStatic
    fun up() {
        CallEvents.registerPushCore { event -> nativeCallEvent(callEventJson(event)) }
        context?.let { pushLog(it, "core: up") }
    }

    /** The app took the core: its bridge hears the calls from now on. */
    @JvmStatic
    fun adopted() {
        context?.let { pushLog(it, "core: adopted by the app") }
    }

    /** The core stopped, or never started (`why`): events wait for the app again. */
    @JvmStatic
    fun stopped(why: String) {
        CallEvents.forgetPushCore()
        IncomingCall.answered = false
        context?.let { pushLog(it, "core: stopped ($why)") }
    }

    /** The core knows who calls: the push's notification says so, quiet if it must. */
    @JvmStatic
    fun ring(caller: String, video: Boolean, muted: Boolean) {
        val context = context ?: return
        InCall.ringingName = caller
        CallRinger.coreRinging = true
        val left = IncomingCall.left(System.currentTimeMillis())
        val shown = pushRing(muted, mayDisturbNow(context), IncomingCall.answered, left)
        pushLog(context, "caller known: " + shown.name.lowercase())
        if (shown == PushRing.NONE) return
        val title = callTitle(caller) ?: context.getString(R.string.ft_someone)
        callNotification(context, title, context.getString(callText(video)), ringing = shown == PushRing.RING, timeoutMs = left, video = video, alertOnce = true)
            ?.let { IncomingCall.update(context, it) }
    }

    /** The call stopped ringing (the caller hung up, it was declined or missed). */
    @JvmStatic
    fun stopRinging() {
        val context = context ?: return
        CallRinger.coreRinging = false
        CallRinger.silence()
        IncomingCall.dismiss(context)
        pushLog(context, "ringing: stopped")
    }

    /** A call refused without a trace (Calls off, a stranger, blocked, a closed session). */
    @JvmStatic
    fun refused() {
        val context = context ?: return
        val cancels = refusalCancels(CallRinger.coreRinging, InCall.active)
        if (cancels) IncomingCall.dismiss(context)
        pushLog(context, if (cancels) "refused: ringing cancelled" else "refused: another call left alone")
    }

    /** The call is over: nothing of it stays on the phone's call screen. */
    @JvmStatic
    fun ended() {
        val context = context ?: return
        Handler(Looper.getMainLooper()).post { InCall.end(context) }
        pushLog(context, "call: ended")
    }
}
