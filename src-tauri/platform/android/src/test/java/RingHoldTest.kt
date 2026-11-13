package com.flickertalk.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RingHoldTest {
    private val t0 = 1_000_000L
    private val left = 45_000L

    @Test
    fun theWindowIsThreeSeconds() {
        assertEquals(3_000L, RING_HOLD_MS)
    }

    @Test
    fun aMutedCallersVerdictInsideTheWindowNeverRings() {
        val hold = RingHold()
        assertEquals(HeldPush.HOLD, hold.pushed(CallPush.RING, t0, left))
        assertEquals(t0 + RING_HOLD_MS, hold.ringAt)
        assertEquals(RingOrder.QUIET, hold.verdict(audible = false))
        assertNull(hold.ringAt)
        assertEquals(RingOrder.NOTHING, hold.due(t0 + RING_HOLD_MS))
        assertEquals(RingOrder.NOTHING, hold.due(t0 + 20_000))
        assertEquals(RingOrder.NOTHING, hold.unheld())
    }

    @Test
    fun aNormalCallersVerdictInsideTheWindowRingsOnceAndAtOnce() {
        val hold = RingHold()
        hold.pushed(CallPush.RING, t0, left)
        assertEquals(RingOrder.RING, hold.verdict(audible = true))
        assertNull(hold.ringAt)
        assertEquals(RingOrder.NOTHING, hold.due(t0 + RING_HOLD_MS))
        assertEquals(RingOrder.NOTHING, hold.verdict(audible = true))
        assertEquals(RingOrder.NOTHING, hold.unheld())
    }

    @Test
    fun withNoVerdictTheCallRingsWhenTheWindowEnds() {
        val hold = RingHold()
        hold.pushed(CallPush.RING, t0, left)
        assertEquals(RingOrder.NOTHING, hold.due(t0 + RING_HOLD_MS - 1))
        assertEquals(t0 + RING_HOLD_MS, hold.ringAt)
        assertEquals(RingOrder.RING, hold.due(t0 + RING_HOLD_MS))
        assertNull(hold.ringAt)
        assertEquals(RingOrder.NOTHING, hold.due(t0 + RING_HOLD_MS + 1))
        assertEquals(RingOrder.NOTHING, hold.verdict(audible = true))
    }

    @Test
    fun aMutedVerdictAfterTheWindowSilencesTheRingAsBefore() {
        val hold = RingHold()
        hold.pushed(CallPush.RING, t0, left)
        assertEquals(RingOrder.RING, hold.due(t0 + RING_HOLD_MS))
        assertEquals(RingOrder.QUIET, hold.verdict(audible = false))
        assertEquals(RingOrder.NOTHING, hold.due(t0 + 2 * RING_HOLD_MS))
    }

    @Test
    fun aCallAnsweredDeclinedOrEndedInsideTheWindowNeverRings() {
        val hold = RingHold()
        hold.pushed(CallPush.RING, t0, left)
        hold.over()
        assertNull(hold.ringAt)
        assertEquals(RingOrder.NOTHING, hold.due(t0 + RING_HOLD_MS))
        assertEquals(RingOrder.NOTHING, hold.verdict(audible = true))
        assertEquals(RingOrder.NOTHING, hold.unheld())
    }

    @Test
    fun whenTheServiceCannotHoldTheProcessTheCallRingsAtOnce() {
        val hold = RingHold()
        hold.pushed(CallPush.RING, t0, left)
        assertEquals(RingOrder.RING, hold.unheld())
        assertNull(hold.ringAt)
        assertEquals(RingOrder.NOTHING, hold.due(t0 + RING_HOLD_MS))
        assertEquals(RingOrder.NOTHING, hold.verdict(audible = true))
    }

    @Test
    fun quietHoursPostTheCallSilentlyWithNoWindow() {
        val hold = RingHold()
        assertEquals(HeldPush.QUIET, hold.pushed(CallPush.SILENT, t0, left))
        assertNull(hold.ringAt)
        assertEquals(RingOrder.NOTHING, hold.unheld())
        assertEquals(RingOrder.NOTHING, hold.due(t0 + RING_HOLD_MS))
        assertEquals(RingOrder.NOTHING, hold.verdict(audible = true))
        assertEquals(RingOrder.QUIET, hold.verdict(audible = false))
    }

    @Test
    fun aSecondPushForTheSameCallDoesNotRestartTheWindow() {
        val hold = RingHold()
        hold.pushed(CallPush.RING, t0, left)
        assertEquals(HeldPush.SAME, hold.pushed(CallPush.RING, t0 + 2_000, left))
        assertEquals(t0 + RING_HOLD_MS, hold.ringAt)
        assertEquals(RingOrder.RING, hold.due(t0 + RING_HOLD_MS))
    }

    @Test
    fun aSecondPushForAMutedCallStaysSilent() {
        val hold = RingHold()
        hold.pushed(CallPush.RING, t0, left)
        hold.verdict(audible = false)
        assertEquals(HeldPush.SAME, hold.pushed(CallPush.RING, t0 + 10_000, left))
        assertNull(hold.ringAt)
        assertEquals(RingOrder.NOTHING, hold.due(t0 + 10_000 + RING_HOLD_MS))
    }

    @Test
    fun aPushAfterTheCallIsOverStartsANewWindow() {
        val hold = RingHold()
        hold.pushed(CallPush.RING, t0, left)
        hold.verdict(audible = false)
        hold.over()
        assertEquals(HeldPush.HOLD, hold.pushed(CallPush.RING, t0 + 20_000, left))
        assertEquals(t0 + 20_000 + RING_HOLD_MS, hold.ringAt)
    }

    @Test
    fun aPushAfterTheLastCallsTimeRanOutStartsANewWindow() {
        val hold = RingHold()
        hold.pushed(CallPush.RING, t0, left)
        hold.verdict(audible = false)
        assertEquals(HeldPush.SAME, hold.pushed(CallPush.RING, t0 + left - 1, left))
        assertEquals(HeldPush.HOLD, hold.pushed(CallPush.RING, t0 + left, left))
    }

    @Test
    fun aCallTheAppRingsItselfIsNeverRungByThePush() {
        val hold = RingHold()
        hold.pushed(CallPush.RING, t0, left)
        hold.appRings(t0 + 1_000)
        assertNull(hold.ringAt)
        assertEquals(RingOrder.NOTHING, hold.due(t0 + RING_HOLD_MS))
        assertEquals(RingOrder.NOTHING, hold.unheld())
        assertEquals(HeldPush.SAME, hold.pushed(CallPush.RING, t0 + 2_000, left))
    }

    @Test
    fun aPushThatComesAfterTheAppAlreadyRangTheCallStaysSilent() {
        val hold = RingHold()
        hold.appRings(t0)
        assertEquals(HeldPush.SAME, hold.pushed(CallPush.RING, t0 + 500, left))
        assertNull(hold.ringAt)
        assertEquals(HeldPush.HOLD, hold.pushed(CallPush.RING, t0 + CALL_RING_MS, left))
    }
}
