package com.flickertalk.platform

import android.app.Activity
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.Ringtone
import android.media.RingtoneManager
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import app.tauri.annotation.InvokeArg

fun refusalCancels(coreRinging: Boolean, inCall: Boolean): Boolean = !coreRinging && !inCall

// same 45 s the router gives the push
const val CALL_RING_MS = 45_000L

fun callRingMillis(sentAt: Long, now: Long): Long {
    val elapsed = now - sentAt
    return if (sentAt > 0 && elapsed in 0 until CALL_RING_MS) CALL_RING_MS - elapsed else CALL_RING_MS
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

data class Ringing(val sound: Boolean, val vibrate: Boolean)

fun ringingFor(ringerMode: Int, quiet: Boolean = false): Ringing = when {
    quiet -> Ringing(sound = false, vibrate = false)
    ringerMode == AudioManager.RINGER_MODE_NORMAL -> Ringing(sound = true, vibrate = true)
    ringerMode == AudioManager.RINGER_MODE_VIBRATE -> Ringing(sound = false, vibrate = true)
    else -> Ringing(sound = false, vibrate = false)
}

internal val RING_PATTERN = longArrayOf(0, 800, 1200)

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

class CoreRinging(private val activity: Activity) {
    private val ringingNotice = RingingNotice(
        post = { call -> showCall(activity, call.title, call.text, video = call.video) },
        cancel = { IncomingCall.dismiss(activity) },
    )

    fun start(args: RingingArgs) {
        InCall.ringingName = args.caller
        CallRinger.coreRinging = true
        showOverLockScreen(activity, overLockScreen(ringing = true, inCall = InCall.active))
        CallRinger.silence()
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
    }

    fun answering(args: AnsweringArgs) {
        InCall.ringingName = args.caller
        CallRinger.coreRinging = false
        ringingNotice.stop()
        CallRinger.silence()
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = true))
    }

    fun refused() {
        if (refusalCancels(CallRinger.coreRinging, InCall.active)) {
            IncomingCall.dismiss(activity)
            CallRinger.silence()
        }
    }

    fun stop() {
        CallRinger.coreRinging = false
        ringingNotice.stop()
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = InCall.active))
        CallRinger.silence()
    }

    fun appHidden() = ringingNotice.appHidden()

    private fun phoneVibrator(): Vibrator? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            activity.getSystemService(VibratorManager::class.java)?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            activity.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        }
}
