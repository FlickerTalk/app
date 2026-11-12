import Foundation
import Tauri

// call media lives in Rust, no WebView in between, so a call answered on a locked iPhone has audio

// mirrors NativeCallEvent in Rust
enum CallEvent: Equatable {
    case incoming
    case answer
    case end
    case decline
    case mute(Bool)
    // call generation, so Rust can drop late events from an older call
    case audioActivated(UInt64)
    case audioDeactivated(UInt64)
    case visible(Bool)
    // UIDeviceOrientation.rawValue
    case orientation(Int)
    case videoRequested
    case route(CallOutput)
}

func callEventPayload(_ event: CallEvent) -> JsonObject {
    switch event {
    case .incoming: return ["event": "incoming"]
    case .answer: return ["event": "answer"]
    case .end: return ["event": "end"]
    case .decline: return ["event": "decline"]
    case .mute(let muted): return ["event": "mute", "muted": muted]
    case .audioActivated(let generation): return ["event": "audioActivated", "generation": generation]
    case .audioDeactivated(let generation): return ["event": "audioDeactivated", "generation": generation]
    case .visible(let visible): return ["event": "visible", "visible": visible]
    case .orientation(let orientation): return ["event": "orientation", "orientation": orientation]
    case .videoRequested: return ["event": "video"]
    case .route(let output): return ["event": "route", "route": output.rawValue]
    }
}

// PushKit can launch the app, and the user answer, before Rust has registered
final class CallEventQueue {
    private let limit: Int
    private var sink: ((CallEvent) -> Void)?
    private var waiting: [CallEvent] = []

    init(limit: Int = 16) {
        self.limit = limit
    }

    func register(_ sink: @escaping (CallEvent) -> Void) {
        self.sink = sink
        let ready = waiting
        waiting = []
        ready.forEach(sink)
    }

    func emit(_ event: CallEvent) {
        if let sink {
            sink(event)
            return
        }
        waiting.append(event)
        if waiting.count > limit { waiting.removeFirst(waiting.count - limit) }
    }

    func forget() {
        waiting = []
    }

    @discardableResult
    func offer(_ event: CallEvent) -> Bool {
        guard let sink else { return false }
        sink(event)
        return true
    }
}

// own queue, so Rust's handler never runs on main and can call the plugin back without a deadlock
final class CallEvents {
    static let shared = CallEvents()
    private let queue = DispatchQueue(label: "com.flickertalk.calls.events")
    private let events = CallEventQueue()

    func register(_ channel: Channel) {
        queue.async { self.events.register { channel.send(callEventPayload($0)) } }
    }

    func emit(_ event: CallEvent) {
        queue.async { self.events.emit(event) }
    }

    func offer(_ event: CallEvent) {
        queue.async { self.events.offer(event) }
    }

    func forget() {
        queue.async { self.events.forget() }
    }
}

struct CallEventsArgs: Decodable {
    let channel: Channel
}

extension PlatformPlugin {
    @objc public func registerCallEvents(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(CallEventsArgs.self)
        CallEvents.shared.register(args.channel)
        DispatchQueue.main.async { AppVisibility.tellNow() }
        invoke.resolve()
    }
}
