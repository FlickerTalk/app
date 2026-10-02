import CallKit
import Foundation

// ---- Opening the app when an incoming call is answered (experiment, 2026-09-29) ----
// An app cannot bring itself to the screen. CallKit does it after the answer only for a call it
// knows as a video call: unlocked, the app opens; locked, the system asks to unlock and then
// opens it. So, with the switch on, every incoming call is reported with `hasVideo`, whatever its
// real media. That media (cameras on or off) stays in the core and in the app's own call screen:
// this only changes what CallKit is told. This phone's own calls are not touched. Off by default;
// the core turns it on (`setOpenAppOnAnswer`, a local debug flag for now).

/// Whether CallKit is told the call has video: an incoming call always, with the switch on; an
/// outgoing call, and any call with the switch off, its real media.
func reportedAsVideo(callHasVideo: Bool, openAppOnAnswer: Bool, outgoing: Bool = false) -> Bool {
    if outgoing { return callHasVideo }
    return callHasVideo || openAppOnAnswer
}

/// The name CallKit shows for a call: the caller's, or, while nobody knows it yet (PushKit reported
/// the call before the core heard of it), "Incoming call" from the app's own tables in the app's
/// language (`Localizable.strings`, one per language of the app).
func incomingCallName(caller: String, bundle: Bundle = .main) -> String {
    caller.isEmpty ? NSLocalizedString("FT_INCOMING_CALL", bundle: bundle, comment: "") : caller
}

/// What CallKit hears when a call is reported, renamed or started: who (`incomingCallName`), and
/// whether it has video (`reportedAsVideo`).
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

/// What `callAnswering` tells CallKit of the incoming call being answered (2026-09-29): its name
/// and, as for every incoming call, `reportedAsVideo`. With the switch on the rename keeps it a
/// video call, so answering still opens the app.
func answeringUpdate(caller: String, video: Bool, openAppOnAnswer: Bool) -> CXCallUpdate {
    callUpdate(caller: caller, video: video, outgoing: false, openAppOnAnswer: openAppOnAnswer)
}

/// What `callVideo` tells CallKit: whether the call has video, as `reportedAsVideo` decides.
func videoUpdate(_ on: Bool, outgoing: Bool, openAppOnAnswer: Bool) -> CXCallUpdate {
    videoUpdate(reportedAsVideo(callHasVideo: on, openAppOnAnswer: openAppOnAnswer, outgoing: outgoing))
}

/// The switch, remembered: a VoIP push can wake the app and must be reported to CallKit at once,
/// before the core has started and said what it wants, so the last choice is used.
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

/// Arguments of `setOpenAppOnAnswer`.
struct OpenAppOnAnswerArgs: Decodable {
    let on: Bool
}
