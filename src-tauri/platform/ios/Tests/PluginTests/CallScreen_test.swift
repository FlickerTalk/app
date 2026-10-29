import XCTest
@testable import tauri_plugin_ft_platform

// The screen stays on for the whole call, voice or video (2026-10-29). WKWebView kept it on while
// it played a call's video; the call's video is native now, so nothing did and the phone dimmed
// and locked mid-call.
final class CallScreenTests: XCTestCase {
    // Awake from CallKit's acceptance of this phone's call, or from the connection, to the end.
    func testTheScreenStaysAwakeWhileTheCallIsLive() {
        XCTAssertTrue(keepsScreenAwake(.started))
        XCTAssertTrue(keepsScreenAwake(.connected))
        XCTAssertFalse(keepsScreenAwake(.ended))
    }

    // The idle timer follows the call through the setter it is given; an end with no call before
    // it only turns the idle timer on again, which it already was.
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
