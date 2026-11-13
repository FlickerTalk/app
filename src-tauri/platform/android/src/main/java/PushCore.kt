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
import android.os.PowerManager
import android.os.SystemClock
import android.util.Log
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import org.json.JSONObject

// A call push starts the core here with no activity (push_core.rs). The app adopts it later, never a second one.

// keep in sync with push_core_handles in push_core.rs
fun pushCoreTakes(event: CallEvent): Boolean =
    event == CallEvent.Incoming || event == CallEvent.Decline || event == CallEvent.End

fun callEventJson(event: CallEvent): String = JSONObject(callEventPayload(event)).toString()

enum class PushRing { NONE, QUIET, RING }

fun pushRing(muted: Boolean, mayDisturb: Boolean, answered: Boolean, leftMs: Long): PushRing = when {
    answered || leftMs <= 0 -> PushRing.NONE
    muted || !mayDisturb -> PushRing.QUIET
    else -> PushRing.RING
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
    private val hold = RingHold()
    private var awake: PowerManager.WakeLock? = null
    private var holding: Context? = null
    private val holdEnds = Runnable { holding?.let { holdEnded(it) } }

    fun pushed(context: Context, push: CallPush, timeoutMs: Long) {
        val app = context.applicationContext
        synchronized(this) {
            val held = hold.pushed(push, SystemClock.elapsedRealtime(), timeoutMs)
            if (held == HeldPush.SAME) {
                pushLog(app, "call push: same call")
                return
            }
            val notification = callNotification(app, app.getString(R.string.ft_someone), app.getString(R.string.ft_incoming_call), timeoutMs = timeoutMs)
            if (notification == null) {
                hold.over()
                return
            }
            this.notification = notification
            deadline = System.currentTimeMillis() + timeoutMs
            showing = true
            answered = false
            generation += 1
            if (held == HeldPush.HOLD) wait(app, RING_HOLD_MS)
            pushLog(app, if (held == HeldPush.HOLD) "call push: holding the ring" else "call push: quiet")
            try {
                ContextCompat.startForegroundService(app, Intent(app, FtIncomingCallService::class.java))
            } catch (_: Exception) {
                // Android 12+ can refuse it once FCM's allowance is used up.
                unheld(app, notification)
            }
        }
    }

    // a process nothing holds can be frozen before the hold ends, so it rings now
    fun unheld(context: Context, shown: Notification) {
        synchronized(this) {
            if (hold.unheld() == RingOrder.RING) {
                pushLog(context, "call push: ringing, nothing holds the process")
                ringNow(context)
            } else if (showing) {
                post(context, shown)
            }
            settle()
        }
    }

    fun known(context: Context, shown: PushRing, caller: String, video: Boolean) {
        synchronized(this) {
            val order = if (shown == PushRing.NONE) RingOrder.NOTHING else hold.verdict(audible = shown == PushRing.RING)
            when (order) {
                RingOrder.RING -> ringNow(context)
                RingOrder.QUIET -> {
                    val title = callTitle(caller) ?: context.getString(R.string.ft_someone)
                    callNotification(context, title, context.getString(callText(video)), timeoutMs = left(System.currentTimeMillis()), video = video, alertOnce = true)
                        ?.let { update(context, it) }
                }
                RingOrder.NOTHING -> {}
            }
            settle()
        }
    }

    fun appRings(context: Context) {
        synchronized(this) {
            hold.appRings(SystemClock.elapsedRealtime())
            pushLog(context, "hold: the app rings the call")
            settle()
        }
    }

    private fun holdEnded(context: Context) {
        synchronized(this) {
            if (hold.due(SystemClock.elapsedRealtime()) == RingOrder.RING) {
                pushLog(context, "hold: no caller yet, ringing")
                ringNow(context)
            }
            hold.ringAt?.let { wait(context, it - SystemClock.elapsedRealtime()) } ?: settle()
        }
    }

    private fun ringNow(context: Context) {
        callNotification(context, context.getString(R.string.ft_someone), context.getString(R.string.ft_incoming_call), ringing = true, timeoutMs = left(System.currentTimeMillis()))
            ?.let { update(context, it) }
    }

    private fun wait(context: Context, delayMs: Long) {
        holding = context
        main.removeCallbacks(holdEnds)
        main.postDelayed(holdEnds, delayMs.coerceAtLeast(0))
        if (awake?.isHeld != true) {
            awake = context.getSystemService(PowerManager::class.java)?.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "FlickerTalk:ringHold")?.apply {
                setReferenceCounted(false)
                acquire(RING_HOLD_MS + 2_000)
            }
        }
    }

    private fun settle() {
        if (hold.ringAt != null) return
        main.removeCallbacks(holdEnds)
        awake?.let { if (it.isHeld) it.release() }
        awake = null
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
        synchronized(this) {
            hold.over()
            settle()
        }
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
        val started = synchronized(IncomingCall) {
            val notification = IncomingCall.notification
                ?: callNotification(this, getString(R.string.ft_someone), getString(R.string.ft_incoming_call))
                ?: return@synchronized false
            try {
                ServiceCompat.startForeground(this, CALL_NOTIFICATION, notification, callServiceTypes(Build.VERSION.SDK_INT, microphone = false))
                true
            } catch (_: Exception) {
                IncomingCall.unheld(this, notification)
                false
            }
        }
        if (!started) {
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
        val shown = pushRing(muted, mayDisturbNow(context), IncomingCall.answered, IncomingCall.left(System.currentTimeMillis()))
        pushLog(context, "caller known: " + shown.name.lowercase())
        IncomingCall.known(context, shown, caller, video)
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
