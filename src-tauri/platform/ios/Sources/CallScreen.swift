import UIKit

// ---- The screen stays on during a call (2026-10-29) ----
// The phone's idle timer dims and locks the screen after its timeout. WKWebView kept the screen on
// while it played a call's video; the call's video is native now (Video.swift), so nothing did and
// the phone locked mid-call. While a call is live, voice or video, the idle timer is off.

/// Where a call is, as far as the screen goes.
enum CallStage {
    /// This phone's call, accepted by CallKit (`startOutgoing`).
    case started
    /// The voice is connected, in either direction (`connected`).
    case connected
    /// Over, however it ended (`ended`).
    case ended
}

/// Whether the screen stays awake: while the call is live.
func keepsScreenAwake(_ stage: CallStage) -> Bool {
    stage != .ended
}

/// Applies `keepsScreenAwake` to the app's idle timer; a test gives its own setter. Main thread.
struct CallScreen {
    let setIdleTimerDisabled: (Bool) -> Void

    init(setIdleTimerDisabled: @escaping (Bool) -> Void = { UIApplication.shared.isIdleTimerDisabled = $0 }) {
        self.setIdleTimerDisabled = setIdleTimerDisabled
    }

    func callIs(_ stage: CallStage) {
        setIdleTimerDisabled(keepsScreenAwake(stage))
    }
}
