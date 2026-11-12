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

// A call push starts the core here with no activity (push_core.rs). The app adopts it later, never a second one.

// keep in sync with push_core_handles in push_core.rs
fun pushCoreTakes(event: CallEvent): Boolean =
    event == CallEvent.Incoming || event == CallEvent.Decline || event == CallEvent.End

fun callEventJson(event: CallEvent): String = JSONObject(callEventPayload(event)).toString()

enum class PushRing { NONE, QUIET }

// Don't post the caller's name over a ringing notification, it stopped the ringtone on a Samsung (Android 13).
fun pushRing(muted: Boolean, mayDisturb: Boolean, answered: Boolean, leftMs: Long): PushRing = when {
    answered || leftMs <= 0 -> PushRing.NONE
    muted || !mayDisturb -> PushRing.QUIET
    else -> PushRing.NONE
}

fun ringLeft(deadline: Long, now: Long): Long = (deadline - now).coerceAtLeast(0)

// debug builds only, and only states, never who calls or an id
fun pushLog(context: Context, line: String) {
    if (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) Log.i("FtPushCore", line)
}

// a foreground service's notification can't be cancelled while it runs, so every end goes through dismiss
object IncomingCall {
    @Volatile
    var notification: Notification? = null
        private set
    @Volatile
    private var deadline = 0L
    @Volatile
    var showing = false
        private set
    @Volatile
    var answered = false
    // a late end from an older push must leave a newer one alone
    @Volatile
    private var generation = 0
    private var service: FtIncomingCallService? = null
    private val main by lazy { Handler(Looper.getMainLooper()) }

    fun ring(context: Context, notification: Notification, timeoutMs: Long) {
        this.notification = notification
        deadline = System.currentTimeMillis() + timeoutMs
        showing = true
        answered = false
        generation += 1
        try {
            ContextCompat.startForegroundService(context, Intent(context, FtIncomingCallService::class.java))
        } catch (_: Exception) {
            // Android 12+ can refuse it once FCM's allowance is used up. It still rings.
            post(context, notification)
        }
    }

    fun update(context: Context, notification: Notification) {
        this.notification = notification
        showing = true
        post(context, notification)
    }

    fun post(context: Context, notification: Notification) {
        try {
            context.getSystemService(NotificationManager::class.java)?.notify(CALL_NOTIFICATION, notification)
        } catch (_: SecurityException) {
            // notifications off, the core still keeps the call
        }
    }

    fun dismiss(context: Context) {
        showing = false
        notification = null
        val ending = generation
        main.post { if (generation == ending) service?.end() }
        context.getSystemService(NotificationManager::class.java)?.cancel(CALL_NOTIFICATION)
    }

    fun left(now: Long): Long = ringLeft(deadline, now)

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

// A process FCM woke is frozen within seconds, core and all, unless this service holds it.
class FtIncomingCallService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // started as a foreground service, so it has to get there even if the call is already gone
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

// Rust (push_core.rs) calls the @JvmStatic functions below by name.
object PushCore {
    const val LIBRARY = "flickertalk_lib"

    @Volatile
    private var context: Context? = null
    @Volatile
    private var loaded = false

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

    @JvmStatic
    external fun nativeCallEvent(json: String): Boolean

    @JvmStatic
    fun openKey(sealed: ByteArray): ByteArray = openStorageKey(sealed)

    @JvmStatic
    fun sealKey(key: ByteArray): ByteArray = sealStorageKey(key)

    @JvmStatic
    fun up() {
        CallEvents.registerPushCore { event -> nativeCallEvent(callEventJson(event)) }
        context?.let { pushLog(it, "core: up") }
    }

    @JvmStatic
    fun adopted() {
        context?.let { pushLog(it, "core: adopted by the app") }
    }

    @JvmStatic
    fun stopped(why: String) {
        CallEvents.forgetPushCore()
        IncomingCall.answered = false
        context?.let { pushLog(it, "core: stopped ($why)") }
    }

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
        callNotification(context, title, context.getString(callText(video)), timeoutMs = left, video = video, alertOnce = true)
            ?.let { IncomingCall.update(context, it) }
    }

    @JvmStatic
    fun stopRinging() {
        val context = context ?: return
        CallRinger.coreRinging = false
        CallRinger.silence()
        IncomingCall.dismiss(context)
        pushLog(context, "ringing: stopped")
    }

    @JvmStatic
    fun refused() {
        val context = context ?: return
        val cancels = refusalCancels(CallRinger.coreRinging, InCall.active)
        if (cancels) IncomingCall.dismiss(context)
        pushLog(context, if (cancels) "refused: ringing cancelled" else "refused: another call left alone")
    }

    @JvmStatic
    fun ended() {
        val context = context ?: return
        Handler(Looper.getMainLooper()).post { InCall.end(context) }
        pushLog(context, "call: ended")
    }
}
