package com.flickertalk.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

class NotificationTapsTest {
    @Test
    fun everyTapHasItsOwnActionAndNoneIsTheLaunchersOwn() {
        val actions = NotificationTap.values().map { it.action }
        assertEquals("one action per tap", actions.size, actions.toSet().size)
        for (action in actions) {
            assertTrue(action, action.startsWith("com.flickertalk.platform."))
            assertFalse(action, action == "android.intent.action.MAIN")
        }
    }

    @Test
    fun anIntentsActionIsReadBackAsItsTap() {
        for (tap in NotificationTap.values()) assertEquals(tap, notificationTapOf(tap.action))
        assertNull(notificationTapOf(null))
        assertNull(notificationTapOf(""))
        assertNull("the launcher's intent is no tap", notificationTapOf("android.intent.action.MAIN"))
        assertNull(notificationTapOf("com.flickertalk.platform.SOMETHING_ELSE"))
    }

    @Test
    fun theCallButtonsAreCallEvents() {
        assertEquals(CallEvent.Answer, tapCallEvent(NotificationTap.CALL_ANSWER))
        assertEquals(CallEvent.Decline, tapCallEvent(NotificationTap.CALL_DECLINE))
        assertEquals(CallEvent.VideoRequested, tapCallEvent(NotificationTap.CALL_VIDEO))
        assertNull("a reminder is not a call", tapCallEvent(NotificationTap.REMINDER))
    }

    @Test
    fun declineOpensTheAppOnlyWhenTheCoreIsNotRunning() {
        assertFalse(tapOpensApp(NotificationTap.CALL_DECLINE, coreListens = true))
        assertTrue(tapOpensApp(NotificationTap.CALL_DECLINE, coreListens = false))
        for (listens in listOf(true, false)) {
            assertTrue(tapOpensApp(NotificationTap.CALL_ANSWER, listens))
            assertTrue(tapOpensApp(NotificationTap.CALL_VIDEO, listens))
            assertTrue(tapOpensApp(NotificationTap.REMINDER, listens))
        }
    }

    @Test
    fun eachTapHasItsOwnRequestCode() {
        val codes = NotificationTap.values().map { tapRequestCode(it) }
        assertEquals(codes.size, codes.toSet().size)
        for (code in codes) assertTrue("$code", code !in 0..7)
    }

    @Test
    fun aTappedReminderIsReadOnce() {
        val reminder = TappedReminder()
        assertEquals("", reminder.take())
        reminder.put("com.flickertalk.notes\nr1")
        assertEquals("com.flickertalk.notes\nr1", reminder.take())
        assertEquals("", reminder.take())
        reminder.put("no reminder key")
        assertEquals("", reminder.take())
        reminder.put(null)
        assertEquals("", reminder.take())
    }

    @Test
    fun theQueueSaysWhetherTheCoreListens() {
        val queue = CallEventQueue()
        assertFalse(queue.listening())
        queue.register {}
        assertTrue(queue.listening())
    }

    private fun activities(manifest: String): List<Element> {
        val nodes = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(File(manifest)).getElementsByTagName("activity")
        return (0 until nodes.length).map { nodes.item(it) as Element }
    }

    @Test
    fun theTapActivityIsPrivateAndShowsOverTheLockScreen() {
        val tap = activities("src/main/AndroidManifest.xml")
            .first { it.getAttribute("android:name") == "com.flickertalk.platform.FtNotificationTapActivity" }
        assertEquals("false", tap.getAttribute("android:exported"))
        assertEquals("true", tap.getAttribute("android:showWhenLocked"))
        assertEquals("true", tap.getAttribute("android:excludeFromRecents"))
        assertEquals("true", tap.getAttribute("android:noHistory"))
        assertTrue("its own task", tap.hasAttribute("android:taskAffinity"))
        assertEquals("", tap.getAttribute("android:taskAffinity"))
    }

    @Test
    fun theDebugNotificationsAreOnlyInDebugBuilds() {
        val main = File("src/main/AndroidManifest.xml").readText()
        assertFalse(main.contains("FtDebugNotificationReceiver"))
        assertTrue(File("src/debug/AndroidManifest.xml").readText().contains("com.flickertalk.platform.FtDebugNotificationReceiver"))
    }
}
