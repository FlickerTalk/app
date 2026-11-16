package com.flickertalk.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class WebViewRecoveryTest {
    @Test
    fun aRendererThatDiesUnderALiveActivityGetsItRecreated() {
        assertEquals(RendererRecovery.RECREATE, rendererRecovery(hasActivity = true, finishing = false, destroyed = false))
    }

    @Test
    fun anActivityOnItsWayOutOnlyLosesTheDeadWebView() {
        assertEquals(RendererRecovery.DESTROY_ONLY, rendererRecovery(hasActivity = false, finishing = false, destroyed = false))
        assertEquals(RendererRecovery.DESTROY_ONLY, rendererRecovery(hasActivity = true, finishing = true, destroyed = false))
        assertEquals(RendererRecovery.DESTROY_ONLY, rendererRecovery(hasActivity = true, finishing = false, destroyed = true))
    }

    @Test
    fun nothingIsCurrentUntilSomethingIsAdopted() {
        assertNull(Latest<Any>().current())
    }

    @Test
    fun theLatestAdoptedOneWins() {
        val second = Any()
        val third = Any()
        val latest = Latest<Any>()
        latest.adopt(second)
        assertSame(second, latest.current())
        latest.adopt(third)
        assertSame(third, latest.current())
    }

    @Test
    fun adoptingSaysWhetherItReplacedAnother() {
        val one = Any()
        val latest = Latest<Any>()
        assertFalse(latest.adopt(one))
        assertFalse(latest.adopt(one))
        assertTrue(latest.adopt(Any()))
    }
}
