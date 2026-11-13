package com.flickertalk.platform

const val RING_HOLD_MS = 3_000L

enum class HeldPush { HOLD, QUIET, SAME }

enum class RingOrder { NOTHING, RING, QUIET }

// The call push says nothing about who calls, so its ring waits a moment for the core to tell.
class RingHold(private val windowMs: Long = RING_HOLD_MS) {
    private enum class State { IDLE, HOLDING, RINGING, QUIET }

    private var state = State.IDLE
    private var holdUntil = 0L
    private var callUntil = 0L

    val ringAt: Long? get() = if (state == State.HOLDING) holdUntil else null

    fun pushed(push: CallPush, now: Long, leftMs: Long): HeldPush {
        if (push == CallPush.IGNORE) return HeldPush.SAME
        if (state != State.IDLE && now < callUntil) return HeldPush.SAME
        callUntil = now + leftMs
        if (push == CallPush.SILENT) {
            state = State.QUIET
            return HeldPush.QUIET
        }
        state = State.HOLDING
        holdUntil = now + windowMs
        return HeldPush.HOLD
    }

    fun unheld(): RingOrder = ringIfHolding()

    fun due(now: Long): RingOrder = if (now >= holdUntil) ringIfHolding() else RingOrder.NOTHING

    fun verdict(audible: Boolean): RingOrder = when {
        state == State.IDLE -> RingOrder.NOTHING
        !audible -> {
            state = State.QUIET
            RingOrder.QUIET
        }
        else -> ringIfHolding()
    }

    fun appRings(now: Long) {
        if (state == State.IDLE || now >= callUntil) callUntil = now + CALL_RING_MS
        state = State.QUIET
    }

    fun over() {
        state = State.IDLE
    }

    private fun ringIfHolding(): RingOrder {
        if (state != State.HOLDING) return RingOrder.NOTHING
        state = State.RINGING
        return RingOrder.RING
    }
}
