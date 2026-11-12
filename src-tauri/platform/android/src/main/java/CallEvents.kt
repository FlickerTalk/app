package com.flickertalk.platform

import app.tauri.plugin.Channel

// mirrors NativeCallEvent in Rust
sealed class CallEvent {
    object Incoming : CallEvent()
    object Answer : CallEvent()
    object End : CallEvent()
    object Decline : CallEvent()
    data class Mute(val muted: Boolean) : CallEvent()
    object AudioActivated : CallEvent()
    object AudioDeactivated : CallEvent()
    data class Visible(val visible: Boolean) : CallEvent()
    data class Orientation(val degrees: Int) : CallEvent()
    object VideoRequested : CallEvent()
    data class Route(val output: CallOutput) : CallEvent()
}

fun callEventPayload(event: CallEvent): Map<String, Any> = when (event) {
    CallEvent.Incoming -> mapOf("event" to "incoming")
    CallEvent.Answer -> mapOf("event" to "answer")
    CallEvent.End -> mapOf("event" to "end")
    CallEvent.Decline -> mapOf("event" to "decline")
    is CallEvent.Mute -> mapOf("event" to "mute", "muted" to event.muted)
    CallEvent.AudioActivated -> mapOf("event" to "audioActivated")
    CallEvent.AudioDeactivated -> mapOf("event" to "audioDeactivated")
    is CallEvent.Visible -> mapOf("event" to "visible", "visible" to event.visible)
    is CallEvent.Orientation -> mapOf("event" to "orientation", "orientation" to event.degrees)
    CallEvent.VideoRequested -> mapOf("event" to "video")
    is CallEvent.Route -> mapOf("event" to "route", "route" to event.output.wire)
}

class CallEventQueue(private val limit: Int = 16) {
    private var sink: ((CallEvent) -> Unit)? = null
    private var pushCore: ((CallEvent) -> Boolean)? = null
    private val waiting = ArrayDeque<CallEvent>()

    @Synchronized
    fun register(sink: (CallEvent) -> Unit) {
        this.sink = sink
        pushCore = null
        while (waiting.isNotEmpty()) sink(waiting.removeFirst())
    }

    @Synchronized
    fun registerPushCore(take: (CallEvent) -> Boolean) {
        if (sink != null) return
        pushCore = take
        val kept = ArrayDeque<CallEvent>()
        while (waiting.isNotEmpty()) {
            val event = waiting.removeFirst()
            if (!toPushCore(event)) kept.addLast(event)
        }
        waiting.addAll(kept)
    }

    @Synchronized
    fun forgetPushCore() {
        pushCore = null
    }

    @Synchronized
    fun emit(event: CallEvent) {
        val listening = sink
        if (listening != null) {
            listening(event)
            return
        }
        if (toPushCore(event)) return
        waiting.addLast(event)
        while (waiting.size > limit) waiting.removeFirst()
    }

    @Synchronized
    fun listening(): Boolean = sink != null

    @Synchronized
    fun hears(event: CallEvent): Boolean = sink != null || (pushCore != null && pushCoreTakes(event))

    @Synchronized
    fun forget() = waiting.clear()

    @Synchronized
    fun offer(event: CallEvent): Boolean {
        val listening = sink
        if (listening != null) {
            listening(event)
            return true
        }
        return toPushCore(event)
    }

    private fun toPushCore(event: CallEvent): Boolean = pushCoreTakes(event) && pushCore?.invoke(event) == true
}

// own thread, so Rust's handler never runs on main and can call the plugin back
object CallEvents {
    private val queue = CallEventQueue()
    private val sender = java.util.concurrent.Executors.newSingleThreadExecutor()

    fun register(channel: Channel) = sender.execute {
        queue.register { channel.sendObject(callEventPayload(it)) }
    }

    fun emit(event: CallEvent) = sender.execute { queue.emit(event) }

    fun offer(event: CallEvent) = sender.execute { queue.offer(event) }

    fun forget() = sender.execute { queue.forget() }

    fun registerPushCore(take: (CallEvent) -> Boolean) = sender.execute { queue.registerPushCore(take) }

    fun forgetPushCore() = sender.execute { queue.forgetPushCore() }

    fun listening(): Boolean = queue.listening()

    fun hears(event: CallEvent): Boolean = queue.hears(event)
}
