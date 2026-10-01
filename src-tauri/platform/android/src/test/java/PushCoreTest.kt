package com.flickertalk.platform

import android.content.pm.ServiceInfo
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import java.lang.reflect.Modifier
import javax.xml.parsers.DocumentBuilderFactory

/**
 * Bug of 2026-10-01 (Samsung, app killed, called from an iPhone): the call push rang "Someone"
 * until the notification's own 45 s limit, long after the caller hung up, and no missed call was
 * kept. The push started the process for `FtMessagingService` alone: the Rust library, and the
 * core with it, only ever started with the app's activity. Now the push starts the core with no
 * activity (`PushCore`, over JNI), which tells the push's notification who calls, stops it when
 * the caller hangs up or the call is refused, and keeps the missed call. A foreground service of
 * type phone call keeps the process awake while it rings.
 */
class PushCoreTest {
    // The core a push started acts on the push itself (reconnect), the notification's decline and a
    // hang-up; an answer waits for the app, which the notification's answer opens. Rust's
    // `push_core_handles` says the same.
    @Test
    fun thePushsCoreTakesThePushADeclineAndAHangUp() {
        assertTrue(pushCoreTakes(CallEvent.Incoming))
        assertTrue(pushCoreTakes(CallEvent.Decline))
        assertTrue(pushCoreTakes(CallEvent.End))
        for (event in listOf(CallEvent.Answer, CallEvent.Mute(true), CallEvent.AudioActivated, CallEvent.Visible(true), CallEvent.VideoRequested)) {
            assertFalse("$event is the app's", pushCoreTakes(event))
        }
    }

    // What travels over JNI is the text the channel carries (Rust's `parse_call_event`).
    @Test
    fun anEventCrossesJniAsTheChannelsText() {
        assertEquals("decline", JSONObject(callEventJson(CallEvent.Decline)).getString("event"))
        assertEquals("end", JSONObject(callEventJson(CallEvent.End)).getString("event"))
        val mute = JSONObject(callEventJson(CallEvent.Mute(true)))
        assertEquals("mute", mute.getString("event"))
        assertTrue(mute.getBoolean("muted"))
    }

    // A decline tapped before the push's core is up waits, and goes to it once it is; an answer
    // keeps waiting for the app. Once the app listens, everything goes to the app, never to both.
    @Test
    fun aDeclineReachesThePushsCoreAndAnAnswerWaitsForTheApp() {
        val queue = CallEventQueue()
        val pushCore = mutableListOf<CallEvent>()
        val app = mutableListOf<CallEvent>()
        queue.emit(CallEvent.Answer)
        queue.emit(CallEvent.Decline)
        queue.registerPushCore { pushCore.add(it); true }
        assertEquals(listOf<CallEvent>(CallEvent.Decline), pushCore)
        queue.emit(CallEvent.End)
        assertEquals(listOf(CallEvent.Decline, CallEvent.End), pushCore)
        queue.register { app.add(it) }
        assertEquals("the answer waited for the app", listOf<CallEvent>(CallEvent.Answer), app)
        queue.emit(CallEvent.Decline)
        assertEquals(listOf(CallEvent.Answer, CallEvent.Decline), app)
        assertEquals("the push's core hears no more", listOf(CallEvent.Decline, CallEvent.End), pushCore)
    }

    // A core that stopped (or never took it) leaves the event waiting for the app.
    @Test
    fun anEventThePushsCoreCannotTakeWaitsForTheApp() {
        val queue = CallEventQueue()
        queue.registerPushCore { false }
        queue.emit(CallEvent.Decline)
        val app = mutableListOf<CallEvent>()
        queue.register { app.add(it) }
        assertEquals(listOf<CallEvent>(CallEvent.Decline), app)
    }

    // The app's core came first: a push's core that comes up later never takes over.
    @Test
    fun thePushsCoreNeverTakesOverFromTheApp() {
        val queue = CallEventQueue()
        val app = mutableListOf<CallEvent>()
        val pushCore = mutableListOf<CallEvent>()
        queue.register { app.add(it) }
        queue.registerPushCore { pushCore.add(it); true }
        queue.emit(CallEvent.Decline)
        assertEquals(listOf<CallEvent>(CallEvent.Decline), app)
        assertTrue(pushCore.isEmpty())
    }

    // The notification's decline opens the app only when no core will hear it; once the push's
    // core stopped, declines wait (and open the app) again.
    @Test
    fun whoHearsADeclineAndAnAnswer() {
        val queue = CallEventQueue()
        assertFalse(queue.hears(CallEvent.Decline))
        queue.registerPushCore { true }
        assertTrue(queue.hears(CallEvent.Decline))
        assertFalse("the answer waits for the app", queue.hears(CallEvent.Answer))
        assertFalse("the app does not listen yet", queue.listening())
        assertTrue("the push reaches the push's core", queue.offer(CallEvent.Incoming))
        assertFalse(queue.offer(CallEvent.Visible(true)))
        queue.forgetPushCore()
        assertFalse(queue.hears(CallEvent.Decline))
        queue.register {}
        assertTrue(queue.hears(CallEvent.Decline))
        assertTrue(queue.hears(CallEvent.Answer))
    }

    // Once the core knows who calls (2026-10-01): a muted contact (app#4) or the weekly hours
    // (app#7) make the call quiet; a call the user already answered from the notification, or
    // whose push has run out, is not shown again.
    @Test
    fun thePushsCallIsShownAsTheCallerAndTheHoursSay() {
        assertEquals(PushRing.RING, pushRing(muted = false, mayDisturb = true, answered = false, leftMs = 30_000))
        assertEquals(PushRing.QUIET, pushRing(muted = true, mayDisturb = true, answered = false, leftMs = 30_000))
        assertEquals(PushRing.QUIET, pushRing(muted = false, mayDisturb = false, answered = false, leftMs = 30_000))
        assertEquals(PushRing.NONE, pushRing(muted = false, mayDisturb = true, answered = true, leftMs = 30_000))
        assertEquals(PushRing.NONE, pushRing(muted = false, mayDisturb = true, answered = false, leftMs = 0))
    }

    // How long the push's call still rings: what is left of its time, never less than nothing.
    @Test
    fun theCallRingsWhatIsLeftOfItsPushsTime() {
        assertEquals(30_000L, ringLeft(deadline = 1_045_000L, now = 1_015_000L))
        assertEquals(0L, ringLeft(deadline = 1_045_000L, now = 1_050_000L))
        assertEquals(0L, ringLeft(deadline = 0L, now = 1_000L))
    }

    // While the push's call rings, a foreground service of type phone call keeps the process (and
    // the core in it) awake: a process FCM woke is frozen within seconds otherwise.
    @Test
    fun theRingingServiceIsAPhoneCallService() {
        assertEquals(ServiceInfo.FOREGROUND_SERVICE_TYPE_PHONE_CALL, callServiceTypes(34, microphone = false))
        val service = elements("service").first { it.getAttribute("android:name") == "com.flickertalk.platform.FtIncomingCallService" }
        assertEquals("false", service.getAttribute("android:exported"))
        assertEquals("phoneCall", service.getAttribute("android:foregroundServiceType"))
    }

    // The library the push loads is the app's own (`[lib] name` of src-tauri/Cargo.toml).
    @Test
    fun thePushLoadsTheAppsLibrary() {
        val cargo = File("../../Cargo.toml").readText()
        assertTrue(cargo.contains("name = \"${PushCore.LIBRARY}\""))
    }

    // The JNI contract with `src-tauri/src/push_core.rs`: the names Rust exports, and the static
    // methods Rust calls back, with the types it passes. A name that drifts fails silently on a
    // phone, so it fails here.
    @Test
    fun theJniNamesMatchRusts() {
        val rust = File("../../src/push_core.rs").readText()
        assertTrue(rust.contains("Java_com_flickertalk_platform_PushCore_nativeStart"))
        assertTrue(rust.contains("Java_com_flickertalk_platform_PushCore_nativeCallEvent"))
        val calls = mapOf(
            "ring" to listOf(String::class.java, Boolean::class.javaPrimitiveType),
            "stopRinging" to emptyList(),
            "refused" to emptyList(),
            "ended" to emptyList(),
            "up" to emptyList(),
            "adopted" to emptyList(),
            "stopped" to listOf(String::class.java),
            "openKey" to listOf(ByteArray::class.java),
            "sealKey" to listOf(ByteArray::class.java),
        )
        for ((name, types) in calls) {
            assertTrue("Rust calls $name", rust.contains("\"$name\""))
            val method = PushCore::class.java.declaredMethods.first { it.name == name }
            assertTrue("$name is static", Modifier.isStatic(method.modifiers))
            if (types.isNotEmpty()) assertEquals(types.first(), method.parameterTypes.first())
        }
        assertEquals(listOf(String::class.java, Boolean::class.javaPrimitiveType, Boolean::class.javaPrimitiveType), PushCore::class.java.declaredMethods.first { it.name == "ring" }.parameterTypes.toList())
        assertTrue(rust.contains("(Ljava/lang/String;ZZ)V"))
        for (native in listOf("nativeStart", "nativeCallEvent")) {
            val method = PushCore::class.java.declaredMethods.first { it.name == native }
            assertTrue("$native is native", Modifier.isNative(method.modifiers))
            assertTrue("$native is static", Modifier.isStatic(method.modifiers))
        }
    }

    // R8 keeps what JNI finds by name in a release build.
    @Test
    fun aReleaseBuildKeepsThePushCoreForJni() {
        val rules = File("../../gen/android/app/proguard-rules.pro").readText()
        assertTrue(rules.contains("-keep class com.flickertalk.platform.PushCore"))
    }

    private fun elements(tag: String): List<Element> {
        val nodes = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(File("src/main/AndroidManifest.xml")).getElementsByTagName(tag)
        return (0 until nodes.length).map { nodes.item(it) as Element }
    }
}
