package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class RemindersTest {
    @Test
    fun thePluginLeavesTheAlarmsToReminders() {
        assertFalse(File("src/main/java/PlatformPlugin.kt").readText().contains("AlarmManager"))
        assertTrue(File("src/main/java/Reminders.kt").readText().contains("AlarmManager"))
    }
}
