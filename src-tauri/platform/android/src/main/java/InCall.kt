package com.flickertalk.platform

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.WindowManager
import androidx.core.content.ContextCompat
import app.tauri.annotation.InvokeArg

fun overLockScreen(ringing: Boolean, inCall: Boolean): Boolean = ringing || inCall

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

fun switchCallVideo(context: Context, on: Boolean) {
    if (InCall.video != on) {
        InCall.video = on
        if (!on) InCall.videoByTap = false
        if (InCall.active) InCall.service?.promote()
    }
    if (on && InCall.active) InCall.preferSpeaker(context)
}

// debuggable builds only, device types only
fun routeLog(context: Context, line: String) {
    if (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) Log.i("FtCallRoute", line)
}

@InvokeArg
class OutgoingArgs {
    var name: String = ""
    var video: Boolean = false
}

@InvokeArg
class SpeakerArgs {
    var on: Boolean = false
}
