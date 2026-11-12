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

class CallRouteTest {
    private val phone = listOf(TYPE_BUILTIN_EARPIECE, TYPE_BUILTIN_SPEAKER)

    @Test
    fun aVoiceCallPlaysOnTheEarpieceOrOnATabletsSpeaker() {
        assertEquals(TYPE_BUILTIN_EARPIECE, callDevice(choice = null, prefersSpeaker = false, available = phone))
        assertEquals(TYPE_BUILTIN_SPEAKER, callDevice(choice = null, prefersSpeaker = false, available = listOf(TYPE_BUILTIN_SPEAKER)))
    }

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

    @Test
    fun theNewestHeadphonesWin() {
        val both = phone + TYPE_BLUETOOTH_SCO + TYPE_WIRED_HEADSET
        assertEquals(TYPE_WIRED_HEADSET, callDevice(null, false, both, newest = TYPE_WIRED_HEADSET))
        assertEquals("not a headset", TYPE_BLUETOOTH_SCO, callDevice(null, false, both, newest = TYPE_BUILTIN_SPEAKER))
        assertEquals("no longer there", TYPE_BLUETOOTH_SCO, callDevice(null, false, both, newest = TYPE_USB_HEADSET))
    }

    @Test
    fun theSpeakerNeverTakesTheCallFromHeadphonesByItself() {
        assertEquals(TYPE_BUILTIN_SPEAKER, callDevice(null, prefersSpeaker = true, available = phone))
        assertEquals(TYPE_WIRED_HEADSET, callDevice(null, prefersSpeaker = true, available = phone + TYPE_WIRED_HEADSET))
        assertEquals(TYPE_BUILTIN_SPEAKER, callDevice(choice = true, prefersSpeaker = false, available = phone + TYPE_BLUETOOTH_SCO))
        assertEquals(TYPE_BLUETOOTH_SCO, callDevice(choice = false, prefersSpeaker = true, available = phone + TYPE_BLUETOOTH_SCO))
        assertEquals(TYPE_BUILTIN_EARPIECE, callDevice(choice = false, prefersSpeaker = true, available = phone))
    }

    @Test
    fun onlyCallHeadsetsAreHeadsets() {
        assertTrue(isHeadset(TYPE_BLUETOOTH_SCO))
        assertTrue(isHeadset(TYPE_WIRED_HEADPHONES))
        assertFalse(isHeadset(TYPE_BLUETOOTH_A2DP))
        assertFalse(isHeadset(TYPE_BUILTIN_SPEAKER))
        assertFalse(isHeadset(TYPE_BUILTIN_EARPIECE))
    }

    @Test
    fun headphonesThatComeInForgetTheSpeakerChoice() {
        assertNull(choiceAfterDevicesAdded(true, listOf(TYPE_WIRED_HEADSET)))
        assertNull(choiceAfterDevicesAdded(false, listOf(TYPE_BLUETOOTH_SCO)))
        assertEquals(true, choiceAfterDevicesAdded(true, listOf(TYPE_BLUETOOTH_A2DP)))
        assertEquals(false, choiceAfterDevicesAdded(false, emptyList()))
    }

    @Test
    fun theCallScreenShowsTheReceiverTheSpeakerOrAHeadset() {
        assertEquals(CallOutput.RECEIVER, outputOf(TYPE_BUILTIN_EARPIECE))
        assertEquals(CallOutput.SPEAKER, outputOf(TYPE_BUILTIN_SPEAKER))
        assertEquals(CallOutput.HEADSET, outputOf(TYPE_BLUETOOTH_SCO))
        assertEquals(CallOutput.HEADSET, outputOf(TYPE_WIRED_HEADPHONES))
    }

    @Test
    fun beforeAndroid12TheRouteIsTheSpeakerphoneOrSco() {
        assertEquals(LegacyRoute.SPEAKERPHONE, legacyRoute(TYPE_BUILTIN_SPEAKER))
        assertEquals(LegacyRoute.BLUETOOTH_SCO, legacyRoute(TYPE_BLUETOOTH_SCO))
        assertEquals(LegacyRoute.SYSTEM, legacyRoute(TYPE_WIRED_HEADSET))
        assertEquals(LegacyRoute.SYSTEM, legacyRoute(TYPE_BUILTIN_EARPIECE))
    }

    @Test
    fun theRouteTravelsToTheCore() {
        assertEquals(mapOf("event" to "route", "route" to "headset"), callEventPayload(CallEvent.Route(CallOutput.HEADSET)))
        assertEquals(mapOf("event" to "route", "route" to "receiver"), callEventPayload(CallEvent.Route(CallOutput.RECEIVER)))
        assertFalse(pushCoreTakes(CallEvent.Route(CallOutput.SPEAKER)))
    }
}
