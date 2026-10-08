package com.flickertalk.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Links that open the app (App Links, 2026-10-08): `MainActivity` hands each intent over before
 * Tauri runs, because Rust registers the plugin later, off the activity's start (the bug of
 * `NotificationTaps.kt`). Only a link the user just opened counts: an activity Android recreated
 * from recents comes back with the old intent, and must not open that link again.
 */
class OpenedLinksTest {
    private val link = "https://flickertalk.com/add#card"

    @Test
    fun aViewIntentOpensItsLink() {
        assertEquals(link, openedLinkOf(VIEW, link, flags = 0, restored = false))
        assertEquals("https://flickertalk.com/move#invite", openedLinkOf(VIEW, "https://flickertalk.com/move#invite", flags = 0, restored = false))
    }

    @Test
    fun theLauncherAndOtherIntentsOpenNoLink() {
        assertNull(openedLinkOf("android.intent.action.MAIN", null, flags = 0, restored = false))
        assertNull(openedLinkOf("android.intent.action.MAIN", link, flags = 0, restored = false))
        assertNull(openedLinkOf(null, link, flags = 0, restored = false))
        assertNull(openedLinkOf(VIEW, null, flags = 0, restored = false))
        assertNull(openedLinkOf(VIEW, "", flags = 0, restored = false))
        assertNull("only the verified https links", openedLinkOf(VIEW, "flickertalk://add#card", flags = 0, restored = false))
        assertNull(openedLinkOf(VIEW, "http://flickertalk.com/add#card", flags = 0, restored = false))
    }

    @Test
    fun anActivityRecreatedFromRecentsDoesNotOpenTheOldLinkAgain() {
        assertNull(openedLinkOf(VIEW, link, flags = FROM_HISTORY, restored = false))
        assertNull(openedLinkOf(VIEW, link, flags = FROM_HISTORY or 0x10000000, restored = false))
        assertNull("restored from its saved state", openedLinkOf(VIEW, link, flags = 0, restored = true))
    }

    @Test
    fun theLinkIsHandedOverOnce() {
        val links = OpenedLinks()
        assertEquals("", links.take())
        links.put(link)
        assertEquals(link, links.take())
        assertEquals("taken already", "", links.take())
    }

    @Test
    fun theLatestLinkWins() {
        val links = OpenedLinks()
        links.put(link)
        links.put("https://flickertalk.com/move#invite")
        assertEquals("https://flickertalk.com/move#invite", links.take())
    }

    @Test
    fun aListenerHearsEachLinkAsItComes() {
        val links = OpenedLinks()
        var heard = 0
        links.put(link)
        links.register { heard++ }
        assertEquals("what came before the listener is asked for, not told", 0, heard)
        links.put(link)
        assertEquals(1, heard)
        links.register { heard += 10 }
        links.put(link)
        assertEquals("a second listener replaces the first", 11, heard)
        assertEquals(link, links.take())
    }

    private companion object {
        const val VIEW = "android.intent.action.VIEW"
        const val FROM_HISTORY = 0x00100000
    }
}
