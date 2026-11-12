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

class PushCoreTest {
    @Test
    fun thePushsCoreTakesThePushADeclineAndAHangUp() {
        assertTrue(pushCoreTakes(CallEvent.Incoming))
        assertTrue(pushCoreTakes(CallEvent.Decline))
        assertTrue(pushCoreTakes(CallEvent.End))
        for (event in listOf(CallEvent.Answer, CallEvent.Mute(true), CallEvent.AudioActivated, CallEvent.Visible(true), CallEvent.VideoRequested)) {
            assertFalse("$event is the app's", pushCoreTakes(event))
        }
    }

    @Test
    fun anEventCrossesJniAsTheChannelsText() {
        assertEquals("decline", JSONObject(callEventJson(CallEvent.Decline)).getString("event"))
        assertEquals("end", JSONObject(callEventJson(CallEvent.End)).getString("event"))
        val mute = JSONObject(callEventJson(CallEvent.Mute(true)))
        assertEquals("mute", mute.getString("event"))
        assertTrue(mute.getBoolean("muted"))
    }

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

    @Test
    fun anEventThePushsCoreCannotTakeWaitsForTheApp() {
        val queue = CallEventQueue()
        queue.registerPushCore { false }
        queue.emit(CallEvent.Decline)
        val app = mutableListOf<CallEvent>()
        queue.register { app.add(it) }
        assertEquals(listOf<CallEvent>(CallEvent.Decline), app)
    }

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

    @Test
    fun thePushsCallIsShownAsTheCallerAndTheHoursSay() {
        assertEquals(PushRing.QUIET, pushRing(muted = true, mayDisturb = true, answered = false, leftMs = 30_000))
        assertEquals(PushRing.QUIET, pushRing(muted = false, mayDisturb = false, answered = false, leftMs = 30_000))
        assertEquals(PushRing.NONE, pushRing(muted = false, mayDisturb = true, answered = true, leftMs = 30_000))
        assertEquals(PushRing.NONE, pushRing(muted = false, mayDisturb = true, answered = false, leftMs = 0))
        assertEquals(PushRing.NONE, pushRing(muted = true, mayDisturb = false, answered = true, leftMs = 30_000))
    }

    @Test
    fun aNormalCallersRingingNotificationIsLeftAlone() {
        assertEquals(PushRing.NONE, pushRing(muted = false, mayDisturb = true, answered = false, leftMs = 30_000))
    }

    @Test
    fun theCallRingsWhatIsLeftOfItsPushsTime() {
        assertEquals(30_000L, ringLeft(deadline = 1_045_000L, now = 1_015_000L))
        assertEquals(0L, ringLeft(deadline = 1_045_000L, now = 1_050_000L))
        assertEquals(0L, ringLeft(deadline = 0L, now = 1_000L))
    }

    @Test
    fun theRingingServiceIsAPhoneCallService() {
        assertEquals(ServiceInfo.FOREGROUND_SERVICE_TYPE_PHONE_CALL, callServiceTypes(34, microphone = false))
        val service = elements("service").first { it.getAttribute("android:name") == "com.flickertalk.platform.FtIncomingCallService" }
        assertEquals("false", service.getAttribute("android:exported"))
        assertEquals("phoneCall", service.getAttribute("android:foregroundServiceType"))
    }

    @Test
    fun thePushLoadsTheAppsLibrary() {
        val cargo = File("../../Cargo.toml").readText()
        assertTrue(cargo.contains("name = \"${PushCore.LIBRARY}\""))
    }

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
