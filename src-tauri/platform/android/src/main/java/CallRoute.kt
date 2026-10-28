package com.flickertalk.platform

import android.annotation.SuppressLint
import android.media.AudioDeviceInfo

// ---- Where a native call's voice plays (2026-10-28) ----
// Headphones take the call: wired or Bluetooth, connected before the call or during it. Bluetooth
// carries a call only over SCO, which Android brings up only for the communication device (or
// `startBluetoothSco` before Android 12); without it the voice stayed on the phone (bug of
// 2026-10-28). The speaker is the user's choice, or a video call's preference over the earpiece,
// never over headphones. The call screen's speaker button follows the route (`CallEvent.Route`).

/** Where the call's voice plays, as the call screen shows it. */
enum class CallOutput(val wire: String) { RECEIVER("receiver"), SPEAKER("speaker"), HEADSET("headset") }

/**
 * Device types that carry a call to headphones, the most wanted first: Bluetooth (as Android's own
 * calls do), then wired and USB. A Bluetooth device that only plays music (A2DP) is not one.
 */
// Compile-time constants: an older Android simply never lists the newer types.
@SuppressLint("InlinedApi")
val HEADSET_TYPES = listOf(
    AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
    AudioDeviceInfo.TYPE_BLE_HEADSET,
    AudioDeviceInfo.TYPE_HEARING_AID,
    AudioDeviceInfo.TYPE_WIRED_HEADSET,
    AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
    AudioDeviceInfo.TYPE_USB_HEADSET,
)

fun isHeadset(type: Int): Boolean = type in HEADSET_TYPES

/**
 * The device type the call's voice goes to, out of the `available` ones: the speaker when the user
 * chose it, or with no choice for a call that had video and no headphones; otherwise headphones
 * (the `newest` that came in, if still there, else by `HEADSET_TYPES`), else the earpiece; a tablet
 * with no earpiece plays on its speaker.
 */
fun callDevice(choice: Boolean?, prefersSpeaker: Boolean, available: List<Int>, newest: Int? = null): Int {
    val headset = newest?.takeIf { isHeadset(it) && it in available } ?: HEADSET_TYPES.firstOrNull { it in available }
    val speaker = choice ?: (prefersSpeaker && headset == null)
    return when {
        speaker -> AudioDeviceInfo.TYPE_BUILTIN_SPEAKER
        headset != null -> headset
        AudioDeviceInfo.TYPE_BUILTIN_EARPIECE in available -> AudioDeviceInfo.TYPE_BUILTIN_EARPIECE
        else -> AudioDeviceInfo.TYPE_BUILTIN_SPEAKER
    }
}

/** Headphones that come in during the call take it, as on a phone call: the speaker choice goes. */
fun choiceAfterDevicesAdded(choice: Boolean?, added: List<Int>): Boolean? = if (added.any(::isHeadset)) null else choice

/** What the call screen shows for a device. */
fun outputOf(type: Int): CallOutput = when (type) {
    AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> CallOutput.SPEAKER
    AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> CallOutput.RECEIVER
    else -> CallOutput.HEADSET
}

/** Before Android 12: the speakerphone switch, SCO started by hand, or what the system picks. */
enum class LegacyRoute { SPEAKERPHONE, BLUETOOTH_SCO, SYSTEM }

/** How a device is reached before Android 12: a wired headset or the earpiece the system picks. */
fun legacyRoute(type: Int): LegacyRoute = when (type) {
    AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> LegacyRoute.SPEAKERPHONE
    AudioDeviceInfo.TYPE_BLUETOOTH_SCO -> LegacyRoute.BLUETOOTH_SCO
    else -> LegacyRoute.SYSTEM
}
