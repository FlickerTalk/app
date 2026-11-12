package com.flickertalk.platform

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class BillingTest {
    @Test
    fun thePluginLeavesPlayBillingToBilling() {
        assertFalse(File("src/main/java/PlatformPlugin.kt").readText().contains("com.android.billingclient"))
        assertTrue(File("src/main/java/Billing.kt").readText().contains("com.android.billingclient"))
    }
}
