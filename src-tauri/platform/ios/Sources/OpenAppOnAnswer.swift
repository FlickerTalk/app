import CallKit
import Foundation
import Tauri

// An app can't bring itself to the screen, CallKit only opens it after answering a video call.
// With the switch on, incoming calls are reported with hasVideo whatever their real media.

func reportedAsVideo(callHasVideo: Bool, openAppOnAnswer: Bool, outgoing: Bool = false) -> Bool {
    if outgoing { return callHasVideo }
    return callHasVideo || openAppOnAnswer
}

// PushKit reports the call before the core knows who it is
func incomingCallName(caller: String, bundle: Bundle = .main) -> String {
    caller.isEmpty ? NSLocalizedString("FT_INCOMING_CALL", bundle: bundle, comment: "") : caller
}

func callUpdate(caller: String, video: Bool, outgoing: Bool, openAppOnAnswer: Bool) -> CXCallUpdate {
    let update = CXCallUpdate()
    let name = incomingCallName(caller: caller)
    update.remoteHandle = CXHandle(type: .generic, value: name)
    update.localizedCallerName = name
    update.hasVideo = reportedAsVideo(callHasVideo: video, openAppOnAnswer: openAppOnAnswer, outgoing: outgoing)
    update.supportsHolding = false
    update.supportsGrouping = false
    update.supportsUngrouping = false
    update.supportsDTMF = false
    return update
}

// the rename has to keep hasVideo, or answering won't open the app
func answeringUpdate(caller: String, video: Bool, openAppOnAnswer: Bool) -> CXCallUpdate {
    callUpdate(caller: caller, video: video, outgoing: false, openAppOnAnswer: openAppOnAnswer)
}

func videoUpdate(_ on: Bool, outgoing: Bool, openAppOnAnswer: Bool) -> CXCallUpdate {
    videoUpdate(reportedAsVideo(callHasVideo: on, openAppOnAnswer: openAppOnAnswer, outgoing: outgoing))
}

// stored, because a VoIP push has to be reported before the core is up
struct OpenAppOnAnswerSetting {
    static let key = "ft.calls.openAppOnAnswer"
    let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    var on: Bool {
        get { defaults.bool(forKey: Self.key) }
        nonmutating set { defaults.set(newValue, forKey: Self.key) }
    }
}

struct OpenAppOnAnswerArgs: Decodable {
    let on: Bool
}

extension PlatformPlugin {
    @objc public func setOpenAppOnAnswer(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(OpenAppOnAnswerArgs.self)
        DispatchQueue.main.async {
            Calls.shared.setOpenAppOnAnswer(args.on)
            invoke.resolve()
        }
    }
}
