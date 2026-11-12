import XCTest
@testable import tauri_plugin_ft_platform

final class CallEventsTests: XCTestCase {
    private func json(_ event: CallEvent) -> NSDictionary {
        NSDictionary(dictionary: callEventPayload(event).mapValues { $0 ?? NSNull() })
    }

    func testCallEventsTravelAsTheCoreReadsThem() {
        XCTAssertEqual(json(.answer), ["event": "answer"])
        XCTAssertEqual(json(.end), ["event": "end"])
        XCTAssertEqual(json(.mute(true)), ["event": "mute", "muted": true])
        XCTAssertEqual(json(.mute(false)), ["event": "mute", "muted": false])
        XCTAssertEqual(json(.audioActivated(3)), ["event": "audioActivated", "generation": 3])
        XCTAssertEqual(json(.audioDeactivated(2)), ["event": "audioDeactivated", "generation": 2])
        XCTAssertTrue(JSONSerialization.isValidJSONObject(callEventPayload(.mute(true)).mapValues { $0 ?? NSNull() }))
    }

    func testCallEventsWaitUntilTheCoreListens() {
        let queue = CallEventQueue()
        var heard: [CallEvent] = []
        queue.emit(.answer)
        queue.emit(.audioActivated(1))
        XCTAssertEqual(heard, [])
        queue.register { heard.append($0) }
        XCTAssertEqual(heard, [.answer, .audioActivated(1)])
        queue.emit(.mute(true))
        XCTAssertEqual(heard, [.answer, .audioActivated(1), .mute(true)])
    }

    func testANewListenerReplacesTheOld() {
        let queue = CallEventQueue()
        var first: [CallEvent] = []
        var second: [CallEvent] = []
        queue.emit(.answer)
        queue.register { first.append($0) }
        queue.register { second.append($0) }
        queue.emit(.end)
        XCTAssertEqual(first, [.answer])
        XCTAssertEqual(second, [.end])
    }

    func testWaitingEventsAreBoundedAndForgottenByANewCall() {
        let queue = CallEventQueue(limit: 2)
        var heard: [CallEvent] = []
        queue.emit(.answer)
        queue.emit(.mute(true))
        queue.emit(.end)
        queue.register { heard.append($0) }
        XCTAssertEqual(heard, [.mute(true), .end], "the latest ones are kept")
        let other = CallEventQueue()
        var none: [CallEvent] = []
        other.emit(.end)
        other.forget()
        other.register { none.append($0) }
        XCTAssertEqual(none, [])
    }

    func testACallPushTellsTheCoreToReconnect() {
        XCTAssertEqual(json(.incoming), ["event": "incoming"])
        let queue = CallEventQueue()
        var heard: [CallEvent] = []
        queue.emit(.end)
        queue.forget()
        queue.emit(.incoming)
        queue.register { heard.append($0) }
        XCTAssertEqual(heard, [.incoming])
    }
}
