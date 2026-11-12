import UIKit

// nothing else keeps the screen awake during a call, the idle timer would lock the phone

enum CallStage {
    case started
    case connected
    case ended
}

func keepsScreenAwake(_ stage: CallStage) -> Bool {
    stage != .ended
}

struct CallScreen {
    let setIdleTimerDisabled: (Bool) -> Void

    init(setIdleTimerDisabled: @escaping (Bool) -> Void = { UIApplication.shared.isIdleTimerDisabled = $0 }) {
        self.setIdleTimerDisabled = setIdleTimerDisabled
    }

    func callIs(_ stage: CallStage) {
        setIdleTimerDisabled(keepsScreenAwake(stage))
    }
}
