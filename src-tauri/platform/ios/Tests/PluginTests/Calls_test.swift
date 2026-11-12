import CallKit
import UIKit
import XCTest
@testable import tauri_plugin_ft_platform

final class CallsTests: XCTestCase {
    private func json(_ event: CallEvent) -> NSDictionary {
        NSDictionary(dictionary: callEventPayload(event).mapValues { $0 ?? NSNull() })
    }

    func testCallKitRingsEveryIncomingCallOncePerCall() {
        XCTAssertEqual(ringWith(callKitCall: false), .report)
        XCTAssertEqual(ringWith(callKitCall: true), .update, "PushKit already reported it: say who it is")
    }

    func testTheAppsAnswerGoesThroughCallKit() {
        XCTAssertTrue(answerThroughCallKit(callKitCall: true, answered: false, outgoing: false))
        XCTAssertFalse(answerThroughCallKit(callKitCall: false, answered: false, outgoing: false), "no CallKit call: the core answers")
        XCTAssertFalse(answerThroughCallKit(callKitCall: true, answered: true, outgoing: false), "already answered")
        XCTAssertFalse(answerThroughCallKit(callKitCall: true, answered: false, outgoing: true))
    }

    func testAnAnsweredCallOutlivesTheRinging() {
        XCTAssertTrue(stopEndsCall(answered: false), "declined or given up: CallKit lets go")
        XCTAssertFalse(stopEndsCall(answered: true), "answered: it goes on until callEnded")
        XCTAssertFalse(stopEndsCall(answered: false, outgoing: true), "this phone's own call is not a ringing one")
    }

    func testConnectingReportsAnOutgoingCall() {
        XCTAssertEqual(connectWith(outgoing: true), .reportConnected)
        XCTAssertEqual(connectWith(outgoing: false), .nothing)
    }

    func testAnEndedCallSaysWhy() {
        XCTAssertEqual(endReason(live: true), .remoteEnded)
        XCTAssertEqual(endReason(live: false), .unanswered)
    }

    func testACallKitCallThatGoesNowhereEndsItself() {
        XCTAssertEqual(callRingLimit, 45, "as the caller's limit and the push's life")
        XCTAssertEqual(callConnectLimit, 30)
        XCTAssertNil(callOverdue(outgoing: false, answered: false, connected: false, ringingFor: 44, answeredFor: nil))
        XCTAssertEqual(callOverdue(outgoing: false, answered: false, connected: false, ringingFor: 46, answeredFor: nil), .unanswered)
        XCTAssertNil(callOverdue(outgoing: false, answered: true, connected: false, ringingFor: 90, answeredFor: 29))
        XCTAssertEqual(callOverdue(outgoing: false, answered: true, connected: false, ringingFor: 90, answeredFor: 31), .failed)
        XCTAssertNil(callOverdue(outgoing: false, answered: true, connected: true, ringingFor: 900, answeredFor: 600))
        XCTAssertNil(callOverdue(outgoing: true, answered: false, connected: false, ringingFor: 600, answeredFor: nil))
    }

    func testTheCallersEndLetsARingingCallKitCallGo() {
        XCTAssertTrue(stopEndsCall(answered: false, outgoing: false))
        XCTAssertEqual(endReason(live: false), .unanswered)
    }

    func testTheProviderConfigurationIsOneCallAtATime() {
        let configuration = callProviderConfiguration(icon: nil)
        XCTAssertTrue(configuration.supportsVideo)
        XCTAssertEqual(configuration.maximumCallsPerCallGroup, 1)
        XCTAssertEqual(configuration.maximumCallGroups, 1)
        XCTAssertEqual(configuration.supportedHandleTypes, [.generic])
        XCTAssertFalse(configuration.includesCallsInRecents)
    }

    func testCallKitsEndOfACallThatNeverConnectedIsADecline() {
        XCTAssertEqual(json(.decline), ["event": "decline"])
        XCTAssertEqual(endEvent(ours: true, outgoing: false, connected: false), .decline)
        XCTAssertEqual(endEvent(ours: true, outgoing: false, connected: true), .end)
        XCTAssertEqual(endEvent(ours: true, outgoing: true, connected: false), .end)
        XCTAssertEqual(endEvent(ours: false, outgoing: false, connected: false), .end, "an older call's end")
    }

    func testCallKitShowsOurMark() {
        let icon = Data([0x89, 0x50, 0x4E, 0x47])
        XCTAssertEqual(callProviderConfiguration(icon: icon).iconTemplateImageData, icon)
        XCTAssertNil(callProviderConfiguration(icon: nil).iconTemplateImageData)
    }

    func testTheCallKitMarkIsATemplateInTheAppsAssets() throws {
        let set = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("gen/apple/Assets.xcassets/\(callKitIconName).imageset")
        let contents = try JSONSerialization.jsonObject(with: Data(contentsOf: set.appendingPathComponent("Contents.json"))) as? [String: Any]
        let properties = contents?["properties"] as? [String: Any]
        XCTAssertEqual(properties?["template-rendering-intent"] as? String, "template")
        let images = contents?["images"] as? [[String: Any]] ?? []
        XCTAssertEqual(Set(images.compactMap { $0["scale"] as? String }), ["1x", "2x", "3x"])
        for image in images {
            let scale = Int((image["scale"] as? String ?? "1x").dropLast()) ?? 1
            let file = try XCTUnwrap(image["filename"] as? String)
            let png = try XCTUnwrap(UIImage(data: Data(contentsOf: set.appendingPathComponent(file))))
            XCTAssertEqual(png.size.width * png.scale, CGFloat(40 * scale))
            XCTAssertEqual(png.size.height * png.scale, CGFloat(40 * scale))
            let alpha = try XCTUnwrap(png.cgImage?.alphaInfo)
            XCTAssertTrue([.premultipliedLast, .last, .premultipliedFirst, .first].contains(alpha), "a transparent background")
        }
    }
}
