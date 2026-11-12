package com.flickertalk.platform

import android.annotation.SuppressLint
import android.media.AudioDeviceInfo

// Bluetooth calls need SCO, which Android only brings up for the communication device (startBluetoothSco before 12)

enum class CallOutput(val wire: String) { RECEIVER("receiver"), SPEAKER("speaker"), HEADSET("headset") }

// inlined constants, an older Android just never reports the newer types
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

fun choiceAfterDevicesAdded(choice: Boolean?, added: List<Int>): Boolean? = if (added.any(::isHeadset)) null else choice

fun outputOf(type: Int): CallOutput = when (type) {
    AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> CallOutput.SPEAKER
    AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> CallOutput.RECEIVER
    else -> CallOutput.HEADSET
}

enum class LegacyRoute { SPEAKERPHONE, BLUETOOTH_SCO, SYSTEM }

fun legacyRoute(type: Int): LegacyRoute = when (type) {
    AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> LegacyRoute.SPEAKERPHONE
    AudioDeviceInfo.TYPE_BLUETOOTH_SCO -> LegacyRoute.BLUETOOTH_SCO
    else -> LegacyRoute.SYSTEM
}
