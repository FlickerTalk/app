package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class CallEventsTest {
    @Test
    fun theCallEventQueueLivesInCallEvents() {
        assertFalse(File("src/main/java/PlatformPlugin.kt").readText().contains("CallEventQueue"))
        assertTrue(File("src/main/java/CallEvents.kt").readText().contains("class CallEventQueue"))
    }
}
