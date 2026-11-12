import AVFAudio
import os

// the speaker override wins over any headphones, so only set it when the user asks or for video on the receiver

enum CallOutput: String, Equatable {
    case receiver
    case speaker
    case headset
}

func callOutput(of outputs: [AVAudioSession.Port]) -> CallOutput {
    if outputs.contains(.builtInSpeaker) { return .speaker }
    if outputs.allSatisfy({ $0 == .builtInReceiver }) { return .receiver }
    return .headset
}

// preferring one of these inputs moves the output there too
let headsetInputs: [AVAudioSession.Port] = [.bluetoothHFP, .headsetMic, .usbAudio, .carAudio]

func headsetInput(inRoute: [AVAudioSession.Port], available: [AVAudioSession.Port]) -> AVAudioSession.Port? {
    if let inUse = inRoute.first(where: headsetInputs.contains) { return inUse }
    return headsetInputs.first(where: available.contains)
}

// wired headphones may have no mic, and the speaker override hides their output
func hasHeadset(outputs: [AVAudioSession.Port], available: [AVAudioSession.Port]) -> Bool {
    callOutput(of: outputs) == .headset || available.contains(where: headsetInputs.contains)
}

func speakerOn(choice: Bool?, prefersSpeaker: Bool, headset: Bool) -> Bool {
    choice ?? (prefersSpeaker && !headset)
}

// the system has already dropped the override when headphones come in
func choiceAfterRouteChange(_ choice: Bool?, reason: AVAudioSession.RouteChangeReason) -> Bool? {
    reason == .newDeviceAvailable ? nil : choice
}

// not after our own override or preferred input, or each change would trigger another
func reroutes(after reason: AVAudioSession.RouteChangeReason) -> Bool {
    switch reason {
    case .newDeviceAvailable, .oldDeviceUnavailable, .categoryChange, .wakeFromSleep, .noSuitableRouteForCategory:
        return true
    default:
        return false
    }
}

// category and mode only, the system can report the options differently than they were set
func isCallSession(category: AVAudioSession.Category, mode: AVAudioSession.Mode) -> Bool {
    category == .playAndRecord && mode == .voiceChat
}

private let routeLogger = Logger(subsystem: "com.flickertalk.calls", category: "route")

// debug only, port types and session names, nothing about the call
func routeLog(_ line: String) {
    #if DEBUG
    routeLogger.notice("ft-route \(line, privacy: .public)")
    #endif
}

func describeSession(_ session: AVAudioSession) -> String {
    let route = session.currentRoute
    let inputs = route.inputs.map(\.portType.rawValue).joined(separator: ",")
    let outputs = route.outputs.map(\.portType.rawValue).joined(separator: ",")
    let available = (session.availableInputs ?? []).map(\.portType.rawValue).joined(separator: ",")
    return "in=[\(inputs)] out=[\(outputs)] available=[\(available)] category=\(session.category.rawValue) mode=\(session.mode.rawValue) options=\(session.categoryOptions.rawValue)"
}
