package com.flickertalk.platform

import android.media.AudioDeviceInfo.TYPE_BLE_HEADSET
import android.media.AudioDeviceInfo.TYPE_BLUETOOTH_A2DP
import android.media.AudioDeviceInfo.TYPE_BLUETOOTH_SCO
import android.media.AudioDeviceInfo.TYPE_BUILTIN_EARPIECE
import android.media.AudioDeviceInfo.TYPE_BUILTIN_SPEAKER
import android.media.AudioDeviceInfo.TYPE_HEARING_AID
import android.media.AudioDeviceInfo.TYPE_USB_HEADSET
import android.media.AudioDeviceInfo.TYPE_WIRED_HEADPHONES
import android.media.AudioDeviceInfo.TYPE_WIRED_HEADSET
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

// Where a native call's voice plays (2026-10-28): headphones, wired or Bluetooth, connected before
// the call or during it, take the call; the speaker is the user's choice, or a video call's
// preference over the earpiece, never over headphones.
class CallRouteTest {
    private val phone = listOf(TYPE_BUILTIN_EARPIECE, TYPE_BUILTIN_SPEAKER)

    // A voice call plays on the earpiece; on a tablet, which has none, on the speaker.
    @Test
    fun aVoiceCallPlaysOnTheEarpieceOrOnATabletsSpeaker() {
        assertEquals(TYPE_BUILTIN_EARPIECE, callDevice(choice = null, prefersSpeaker = false, available = phone))
        assertEquals(TYPE_BUILTIN_SPEAKER, callDevice(choice = null, prefersSpeaker = false, available = listOf(TYPE_BUILTIN_SPEAKER)))
    }

    // The bug of 2026-10-28: a Bluetooth headset was never the call's device (it needs SCO, which
    // only a communication device or `startBluetoothSco` brings up), so the voice stayed on the
    // phone. Any headset takes the call; Bluetooth first, as Android's own calls do.
    @Test
    fun headphonesTakeTheCall() {
        assertEquals(TYPE_BLUETOOTH_SCO, callDevice(null, false, phone + TYPE_BLUETOOTH_SCO))
        assertEquals(TYPE_WIRED_HEADSET, callDevice(null, false, phone + TYPE_WIRED_HEADSET))
        assertEquals(TYPE_WIRED_HEADPHONES, callDevice(null, false, phone + TYPE_WIRED_HEADPHONES))
        assertEquals(TYPE_USB_HEADSET, callDevice(null, false, phone + TYPE_USB_HEADSET))
        assertEquals(TYPE_BLE_HEADSET, callDevice(null, false, phone + TYPE_BLE_HEADSET))
        assertEquals(TYPE_HEARING_AID, callDevice(null, false, phone + TYPE_HEARING_AID))
        assertEquals(TYPE_BLUETOOTH_SCO, callDevice(null, false, phone + TYPE_WIRED_HEADSET + TYPE_BLUETOOTH_SCO))
    }

    // Headphones that just came in win over others already there.
    @Test
    fun theNewestHeadphonesWin() {
        val both = phone + TYPE_BLUETOOTH_SCO + TYPE_WIRED_HEADSET
        assertEquals(TYPE_WIRED_HEADSET, callDevice(null, false, both, newest = TYPE_WIRED_HEADSET))
        assertEquals("not a headset", TYPE_BLUETOOTH_SCO, callDevice(null, false, both, newest = TYPE_BUILTIN_SPEAKER))
        assertEquals("no longer there", TYPE_BLUETOOTH_SCO, callDevice(null, false, both, newest = TYPE_USB_HEADSET))
    }

    // The speaker: the user's choice, even with headphones; with no choice, a call that had video
    // prefers it to the earpiece, never to headphones.
    @Test
    fun theSpeakerNeverTakesTheCallFromHeadphonesByItself() {
        assertEquals(TYPE_BUILTIN_SPEAKER, callDevice(null, prefersSpeaker = true, available = phone))
        assertEquals(TYPE_WIRED_HEADSET, callDevice(null, prefersSpeaker = true, available = phone + TYPE_WIRED_HEADSET))
        assertEquals(TYPE_BUILTIN_SPEAKER, callDevice(choice = true, prefersSpeaker = false, available = phone + TYPE_BLUETOOTH_SCO))
        assertEquals(TYPE_BLUETOOTH_SCO, callDevice(choice = false, prefersSpeaker = true, available = phone + TYPE_BLUETOOTH_SCO))
        assertEquals(TYPE_BUILTIN_EARPIECE, callDevice(choice = false, prefersSpeaker = true, available = phone))
    }

    // A Bluetooth headset that only plays music (A2DP) is no call device.
    @Test
    fun onlyCallHeadsetsAreHeadsets() {
        assertTrue(isHeadset(TYPE_BLUETOOTH_SCO))
        assertTrue(isHeadset(TYPE_WIRED_HEADPHONES))
        assertFalse(isHeadset(TYPE_BLUETOOTH_A2DP))
        assertFalse(isHeadset(TYPE_BUILTIN_SPEAKER))
        assertFalse(isHeadset(TYPE_BUILTIN_EARPIECE))
    }

    // Headphones that come in during the call take it, as on a phone call: the user's speaker
    // choice is forgotten. Other devices leave it.
    @Test
    fun headphonesThatComeInForgetTheSpeakerChoice() {
        assertNull(choiceAfterDevicesAdded(true, listOf(TYPE_WIRED_HEADSET)))
        assertNull(choiceAfterDevicesAdded(false, listOf(TYPE_BLUETOOTH_SCO)))
        assertEquals(true, choiceAfterDevicesAdded(true, listOf(TYPE_BLUETOOTH_A2DP)))
        assertEquals(false, choiceAfterDevicesAdded(false, emptyList()))
    }

    // What the call screen shows for each device.
    @Test
    fun theCallScreenShowsTheReceiverTheSpeakerOrAHeadset() {
        assertEquals(CallOutput.RECEIVER, outputOf(TYPE_BUILTIN_EARPIECE))
        assertEquals(CallOutput.SPEAKER, outputOf(TYPE_BUILTIN_SPEAKER))
        assertEquals(CallOutput.HEADSET, outputOf(TYPE_BLUETOOTH_SCO))
        assertEquals(CallOutput.HEADSET, outputOf(TYPE_WIRED_HEADPHONES))
    }

    // Before Android 12 there is no communication device: the speakerphone switch, and Bluetooth
    // SCO started by hand for a Bluetooth headset; a wired one the system picks by itself.
    @Test
    fun beforeAndroid12TheRouteIsTheSpeakerphoneOrSco() {
        assertEquals(LegacyRoute.SPEAKERPHONE, legacyRoute(TYPE_BUILTIN_SPEAKER))
        assertEquals(LegacyRoute.BLUETOOTH_SCO, legacyRoute(TYPE_BLUETOOTH_SCO))
        assertEquals(LegacyRoute.SYSTEM, legacyRoute(TYPE_WIRED_HEADSET))
        assertEquals(LegacyRoute.SYSTEM, legacyRoute(TYPE_BUILTIN_EARPIECE))
    }

    // The call screen hears the route through the core, as `{"event": "route", "route": …}`.
    @Test
    fun theRouteTravelsToTheCore() {
        assertEquals(mapOf("event" to "route", "route" to "headset"), callEventPayload(CallEvent.Route(CallOutput.HEADSET)))
        assertEquals(mapOf("event" to "route", "route" to "receiver"), callEventPayload(CallEvent.Route(CallOutput.RECEIVER)))
        assertFalse(pushCoreTakes(CallEvent.Route(CallOutput.SPEAKER)))
    }
}
