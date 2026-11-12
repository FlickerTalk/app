import CallKit
import XCTest
@testable import tauri_plugin_ft_platform

final class OpenAppOnAnswerTests: XCTestCase {
    private func freshDefaults() -> UserDefaults {
        let suite = "ft.tests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        addTeardownBlock { defaults.removePersistentDomain(forName: suite) }
        return defaults
    }

    func testTheSwitchMakesEveryIncomingCallAVideoCall() {
        XCTAssertFalse(reportedAsVideo(callHasVideo: false, openAppOnAnswer: false))
        XCTAssertTrue(reportedAsVideo(callHasVideo: true, openAppOnAnswer: false))
        XCTAssertTrue(reportedAsVideo(callHasVideo: false, openAppOnAnswer: true))
        XCTAssertTrue(reportedAsVideo(callHasVideo: true, openAppOnAnswer: true))
        XCTAssertFalse(reportedAsVideo(callHasVideo: false, openAppOnAnswer: true, outgoing: true))
        XCTAssertTrue(reportedAsVideo(callHasVideo: true, openAppOnAnswer: true, outgoing: true))
    }

    func testAPushKitReportIsAVideoCallWithTheSwitchOn() {
        let on = callUpdate(caller: "", video: false, outgoing: false, openAppOnAnswer: true)
        XCTAssertTrue(on.hasVideo)
        XCTAssertEqual(on.localizedCallerName, NSLocalizedString("FT_INCOMING_CALL", comment: ""))
        XCTAssertFalse(callUpdate(caller: "", video: false, outgoing: false, openAppOnAnswer: false).hasVideo)
    }

    func testRingingNamesTheCallerAndKeepsItAVideoCall() {
        let ringing = callUpdate(caller: "Alice", video: false, outgoing: false, openAppOnAnswer: true)
        XCTAssertTrue(ringing.hasVideo)
        XCTAssertEqual(ringing.localizedCallerName, "Alice")
        XCTAssertEqual(ringing.remoteHandle?.value, "Alice")
        XCTAssertFalse(ringing.supportsHolding)
        XCTAssertFalse(ringing.supportsDTMF)
        XCTAssertTrue(callUpdate(caller: "Alice", video: true, outgoing: false, openAppOnAnswer: false).hasVideo)
    }

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

    func testAnOutgoingCallTellsItsRealVideo() {
        XCTAssertFalse(callUpdate(caller: "Alice", video: false, outgoing: true, openAppOnAnswer: true).hasVideo)
        XCTAssertTrue(callUpdate(caller: "Alice", video: true, outgoing: true, openAppOnAnswer: true).hasVideo)
    }

    func testCallVideoKeepsAnIncomingCallAVideoCallWithTheSwitchOn() {
        XCTAssertTrue(videoUpdate(false, outgoing: false, openAppOnAnswer: true).hasVideo)
        XCTAssertFalse(videoUpdate(false, outgoing: false, openAppOnAnswer: false).hasVideo)
        XCTAssertFalse(videoUpdate(false, outgoing: true, openAppOnAnswer: true).hasVideo)
        XCTAssertTrue(videoUpdate(true, outgoing: true, openAppOnAnswer: false).hasVideo)
        XCTAssertNil(videoUpdate(false, outgoing: false, openAppOnAnswer: true).localizedCallerName)
    }

    func testTheSwitchIsOffByDefaultAndRemembered() {
        let defaults = freshDefaults()
        XCTAssertFalse(OpenAppOnAnswerSetting(defaults: defaults).on)
        OpenAppOnAnswerSetting(defaults: defaults).on = true
        XCTAssertTrue(OpenAppOnAnswerSetting(defaults: defaults).on)
        OpenAppOnAnswerSetting(defaults: defaults).on = false
        XCTAssertFalse(OpenAppOnAnswerSetting(defaults: defaults).on)
    }

    func testTheCommandReadsTheSwitchAsRustSendsIt() throws {
        let args = try JSONDecoder().decode(OpenAppOnAnswerArgs.self, from: Data(#"{"on":true}"#.utf8))
        XCTAssertTrue(args.on)
    }
}
