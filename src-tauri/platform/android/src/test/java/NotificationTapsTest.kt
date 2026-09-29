package com.flickertalk.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

/**
 * Bug of 2026-09-29 (Lenovo tablet, Android 16): with the app's process killed and its task in
 * recents, "Answer" on the call notification opened the app but the call was never answered. The
 * answer rode as an extra on the launcher's intent; Android handed it to the recreated activity as
 * a new intent before Tauri had registered the plugin (Rust registers it later, off the activity's
 * start), so no one read it, and the plugin then read the activity's first intent, which had none.
 * The notification's buttons now go to an activity of the bridge's own, which hands the tap to the
 * process at once (the call events queue, the tapped reminder) and then opens the app if it must.
 */
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

    // The call's buttons are what CallKit's are: events for the core, never for the WebView.
    @Test
    fun theCallButtonsAreCallEvents() {
        assertEquals(CallEvent.Answer, tapCallEvent(NotificationTap.CALL_ANSWER))
        assertEquals(CallEvent.Decline, tapCallEvent(NotificationTap.CALL_DECLINE))
        assertEquals(CallEvent.VideoRequested, tapCallEvent(NotificationTap.CALL_VIDEO))
        assertNull("a reminder is not a call", tapCallEvent(NotificationTap.REMINDER))
    }

    // Answering opens the app (the microphone may need asking, the video needs the screen), and so
    // do the camera and a reminder. Declining does not, unless the core is not running: only the
    // core can tell the caller, and it runs with the app (Tauri starts Rust with the activity).
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

    // Each button's PendingIntent is its own: distinct request codes, clear of the ones the other
    // notifications use (0-7), so FLAG_UPDATE_CURRENT never rewrites another button's intent.
    @Test
    fun eachTapHasItsOwnRequestCode() {
        val codes = NotificationTap.values().map { tapRequestCode(it) }
        assertEquals(codes.size, codes.toSet().size)
        for (code in codes) assertTrue("$code", code !in 0..7)
    }

    // The reminder the user tapped waits in the process until the app asks for it, once.
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

    // A decline stays out of the app only when the core already listens.
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

    // The tap's activity is private (no other app answers a call for the user), shows over the
    // lock screen (declining or answering needs no unlock), and leaves no trace: its own task, out
    // of recents, gone once it has handed the tap over.
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

    // The debug-only way to post the notifications (device checks, no real call) never ships.
    @Test
    fun theDebugNotificationsAreOnlyInDebugBuilds() {
        val main = File("src/main/AndroidManifest.xml").readText()
        assertFalse(main.contains("FtDebugNotificationReceiver"))
        assertTrue(File("src/debug/AndroidManifest.xml").readText().contains("com.flickertalk.platform.FtDebugNotificationReceiver"))
    }
}
