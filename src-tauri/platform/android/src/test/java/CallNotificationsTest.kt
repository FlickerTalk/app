package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class CallNotificationsTest {
    @Test
    fun thePluginLeavesTheIncomingCallNotificationToCallNotifications() {
        val plugin = File("src/main/java/PlatformPlugin.kt").readText()
        val notifications = File("src/main/java/CallNotifications.kt").readText()
        for (api in listOf("CallStyle.forIncomingCall", "ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT")) {
            assertFalse(api, plugin.contains(api))
            assertTrue(api, notifications.contains(api))
        }
    }
}
