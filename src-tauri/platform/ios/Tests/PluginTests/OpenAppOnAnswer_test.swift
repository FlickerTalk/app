import CallKit
import XCTest
@testable import tauri_plugin_ft_platform

// Opening the app when an incoming call is answered (experiment, 2026-09-29): iOS brings the app
// to the screen after the answer only for a call CallKit knows as a video call, so with the switch
// on every incoming call is reported with `hasVideo`. The call's real media stays in the core.
final class OpenAppOnAnswerTests: XCTestCase {
    private func freshDefaults() -> UserDefaults {
        let suite = "ft.tests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        addTeardownBlock { defaults.removePersistentDomain(forName: suite) }
        return defaults
    }

    // With the switch on, an incoming call is a video call for CallKit, whatever its media. Off,
    // CallKit hears the truth. An outgoing call always tells the truth.
    func testTheSwitchMakesEveryIncomingCallAVideoCall() {
        XCTAssertFalse(reportedAsVideo(callHasVideo: false, openAppOnAnswer: false))
        XCTAssertTrue(reportedAsVideo(callHasVideo: true, openAppOnAnswer: false))
        XCTAssertTrue(reportedAsVideo(callHasVideo: false, openAppOnAnswer: true))
        XCTAssertTrue(reportedAsVideo(callHasVideo: true, openAppOnAnswer: true))
        XCTAssertFalse(reportedAsVideo(callHasVideo: false, openAppOnAnswer: true, outgoing: true))
        XCTAssertTrue(reportedAsVideo(callHasVideo: true, openAppOnAnswer: true, outgoing: true))
    }

    // PushKit reports the call at once, before anyone knows who calls or with what: with the
    // switch on it is already a video call, so answering it opens the app.
    func testAPushKitReportIsAVideoCallWithTheSwitchOn() {
        let on = callUpdate(caller: "", video: false, outgoing: false, openAppOnAnswer: true)
        XCTAssertTrue(on.hasVideo)
        XCTAssertEqual(on.localizedCallerName, NSLocalizedString("FT_INCOMING_CALL", comment: ""))
        XCTAssertFalse(callUpdate(caller: "", video: false, outgoing: false, openAppOnAnswer: false).hasVideo)
    }

    // `startRinging` (a new report, or the name of the call PushKit reported) keeps the caller's
    // name and, with the switch on, the video flag of a voice call.
    func testRingingNamesTheCallerAndKeepsItAVideoCall() {
        let ringing = callUpdate(caller: "Alice", video: false, outgoing: false, openAppOnAnswer: true)
        XCTAssertTrue(ringing.hasVideo)
        XCTAssertEqual(ringing.localizedCallerName, "Alice")
        XCTAssertEqual(ringing.remoteHandle?.value, "Alice")
        XCTAssertFalse(ringing.supportsHolding)
        XCTAssertFalse(ringing.supportsDTMF)
        XCTAssertTrue(callUpdate(caller: "Alice", video: true, outgoing: false, openAppOnAnswer: false).hasVideo)
    }

    // `callAnswering` (2026-09-29, `fix-bridge`): the call answered in CallKit, maybe before its
    // offer came, gets its caller's name. With the switch on it must stay a video call for
    // CallKit, or the rename of a voice call would take back what opens the app; off, it tells the
    // call's real media, exactly as without the experiment.
    func testTheAnsweredCallsRenameKeepsItAVideoCallWithTheSwitchOn() {
        let on = answeringUpdate(caller: "Alice", video: false, openAppOnAnswer: true)
        XCTAssertTrue(on.hasVideo)
        XCTAssertEqual(on.localizedCallerName, "Alice")
        XCTAssertEqual(on.remoteHandle?.value, "Alice")
        XCTAssertFalse(answeringUpdate(caller: "Alice", video: false, openAppOnAnswer: false).hasVideo)
        XCTAssertTrue(answeringUpdate(caller: "Alice", video: true, openAppOnAnswer: false).hasVideo)
        XCTAssertEqual(answeringUpdate(caller: "", video: false, openAppOnAnswer: true).localizedCallerName,
                       NSLocalizedString("FT_INCOMING_CALL", comment: ""))
    }

    // This phone's own calls are not touched: CallKit hears their real media.
    func testAnOutgoingCallTellsItsRealVideo() {
        XCTAssertFalse(callUpdate(caller: "Alice", video: false, outgoing: true, openAppOnAnswer: true).hasVideo)
        XCTAssertTrue(callUpdate(caller: "Alice", video: true, outgoing: true, openAppOnAnswer: true).hasVideo)
    }

    // `callVideo`: turning the cameras off does not turn an incoming call back into a voice call
    // while the switch is on (the unlock must still open the app); the rest hear the truth.
    func testCallVideoKeepsAnIncomingCallAVideoCallWithTheSwitchOn() {
        XCTAssertTrue(videoUpdate(false, outgoing: false, openAppOnAnswer: true).hasVideo)
        XCTAssertFalse(videoUpdate(false, outgoing: false, openAppOnAnswer: false).hasVideo)
        XCTAssertFalse(videoUpdate(false, outgoing: true, openAppOnAnswer: true).hasVideo)
        XCTAssertTrue(videoUpdate(true, outgoing: true, openAppOnAnswer: false).hasVideo)
        XCTAssertNil(videoUpdate(false, outgoing: false, openAppOnAnswer: true).localizedCallerName)
    }

    // Off unless the core turned it on; remembered, so a call PushKit reports before the core
    // starts (the app was not running) follows the last choice.
    func testTheSwitchIsOffByDefaultAndRemembered() {
        let defaults = freshDefaults()
        XCTAssertFalse(OpenAppOnAnswerSetting(defaults: defaults).on)
        OpenAppOnAnswerSetting(defaults: defaults).on = true
        XCTAssertTrue(OpenAppOnAnswerSetting(defaults: defaults).on)
        OpenAppOnAnswerSetting(defaults: defaults).on = false
        XCTAssertFalse(OpenAppOnAnswerSetting(defaults: defaults).on)
    }

    // What Rust's `set_open_app_on_answer` sends.
    func testTheCommandReadsTheSwitchAsRustSendsIt() throws {
        let args = try JSONDecoder().decode(OpenAppOnAnswerArgs.self, from: Data(#"{"on":true}"#.utf8))
        XCTAssertTrue(args.on)
    }
}
