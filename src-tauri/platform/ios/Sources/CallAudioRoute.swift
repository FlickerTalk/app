import AVFAudio
import os

// ---- Where a native call's voice plays (2026-10-28) ----
// Headphones take the call: wired or Bluetooth, connected before the call or during it. The
// session allows them (`callAudioOptions`); the speaker, an override of the output, beats any of
// them, so it goes on only when the user asks for it, or for a call that had video when the voice
// would otherwise be on the receiver. Each route change decides again, and the call screen's
// speaker button follows the route (`CallEvent.route`). Bug of 2026-10-28: with headphones on, a
// call kept playing on the iPhone.

/// Where the call's voice plays, as the call screen shows it.
enum CallOutput: String, Equatable {
    case receiver
    case speaker
    case headset
}

/// The output of a route: the iPhone's speaker or receiver, or anything else it plays to (wired
/// headphones, Bluetooth, USB, a car), which the call screen shows as a headset.
func callOutput(of outputs: [AVAudioSession.Port]) -> CallOutput {
    if outputs.contains(.builtInSpeaker) { return .speaker }
    if outputs.allSatisfy({ $0 == .builtInReceiver }) { return .receiver }
    return .headset
}

/// Inputs that come with headphones, the most wanted first: Bluetooth hands-free, the microphone
/// of wired headphones, USB, a car. Choosing one takes the call's voice there both ways.
let headsetInputs: [AVAudioSession.Port] = [.bluetoothHFP, .headsetMic, .usbAudio, .carAudio]

/// The headset microphone the call uses: the one the route has already, else the first one there
/// is by `headsetInputs`; `nil` with only the iPhone's own.
func headsetInput(inRoute: [AVAudioSession.Port], available: [AVAudioSession.Port]) -> AVAudioSession.Port? {
    if let inUse = inRoute.first(where: headsetInputs.contains) { return inUse }
    return headsetInputs.first(where: available.contains)
}

/// Whether headphones are there: the route plays to them (wired headphones may have no
/// microphone), or a headset microphone is available (the speaker override hides their output).
func hasHeadset(outputs: [AVAudioSession.Port], available: [AVAudioSession.Port]) -> Bool {
    callOutput(of: outputs) == .headset || available.contains(where: headsetInputs.contains)
}

/// Whether the speaker override goes on: the user's choice in this call, if any; with none, a call
/// that had video prefers the speaker to the receiver, never to headphones.
func speakerOn(choice: Bool?, prefersSpeaker: Bool, headset: Bool) -> Bool {
    choice ?? (prefersSpeaker && !headset)
}

/// The user's speaker choice after a route change: headphones that come in take the call, as on a
/// phone call (the system has dropped the override already); any other change keeps it.
func choiceAfterRouteChange(_ choice: Bool?, reason: AVAudioSession.RouteChangeReason) -> Bool? {
    reason == .newDeviceAvailable ? nil : choice
}

/// Whether the route is decided again after a change: a device came or went, the category changed
/// under the call, or the system moved it. Never after our own override or preferred input, which
/// would answer each change with another.
func reroutes(after reason: AVAudioSession.RouteChangeReason) -> Bool {
    switch reason {
    case .newDeviceAvailable, .oldDeviceUnavailable, .categoryChange, .wakeFromSleep, .noSuitableRouteForCategory:
        return true
    default:
        return false
    }
}

/// Whether the session is still the call's, by its category and mode as `configureCallAudio` set
/// them: something else in the app (the WebView's media) may change it during a call. Not by its
/// options, which the system may report otherwise than they were set.
func isCallSession(category: AVAudioSession.Category, mode: AVAudioSession.Mode) -> Bool {
    category == .playAndRecord && mode == .voiceChat
}

private let routeLogger = Logger(subsystem: "com.flickertalk.calls", category: "route")

/// One line per route decision, for checking on a device (Console, or `idevicesyslog -m ft-route`):
/// only port types and the session's names, nothing about the call or the people in it.
func routeLog(_ line: String) {
    #if DEBUG
    routeLogger.notice("ft-route \(line, privacy: .public)")
    #endif
}

/// The session as `routeLog` writes it.
func describeSession(_ session: AVAudioSession) -> String {
    let route = session.currentRoute
    let inputs = route.inputs.map(\.portType.rawValue).joined(separator: ",")
    let outputs = route.outputs.map(\.portType.rawValue).joined(separator: ",")
    let available = (session.availableInputs ?? []).map(\.portType.rawValue).joined(separator: ",")
    return "in=[\(inputs)] out=[\(outputs)] available=[\(available)] category=\(session.category.rawValue) mode=\(session.mode.rawValue) options=\(session.categoryOptions.rawValue)"
}
