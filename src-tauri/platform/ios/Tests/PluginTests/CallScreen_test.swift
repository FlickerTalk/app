import XCTest
@testable import tauri_plugin_ft_platform

final class CallScreenTests: XCTestCase {
    func testTheScreenStaysAwakeWhileTheCallIsLive() {
        XCTAssertTrue(keepsScreenAwake(.started))
        XCTAssertTrue(keepsScreenAwake(.connected))
        XCTAssertFalse(keepsScreenAwake(.ended))
    }

    func testTheIdleTimerFollowsTheCall() {
        var disabled: [Bool] = []
        let screen = CallScreen(setIdleTimerDisabled: { disabled.append($0) })
        screen.callIs(.ended)
        screen.callIs(.started)
        screen.callIs(.connected)
        screen.callIs(.ended)
        XCTAssertEqual(disabled, [false, true, true, false])
    }
}
