import CallKit
import UserNotifications
import XCTest
@testable import tauri_plugin_ft_platform

final class QuietCallsTests: XCTestCase {
    func testTheWeeklyHoursAreReadAsKotlinReadsThem() {
        XCTAssertTrue(mayDisturb(week: "", day: 3, minute: 600), "no hours: always")
        let week = "540-1080;all;none;1320-420;540-1080;all;all"
        XCTAssertTrue(mayDisturb(week: week, day: 0, minute: 540))
        XCTAssertFalse(mayDisturb(week: week, day: 0, minute: 1080), "the end is outside")
        XCTAssertFalse(mayDisturb(week: week, day: 0, minute: 300))
        XCTAssertTrue(mayDisturb(week: week, day: 1, minute: 0))
        XCTAssertFalse(mayDisturb(week: week, day: 2, minute: 720))
        XCTAssertTrue(mayDisturb(week: week, day: 3, minute: 1400), "past midnight: late")
        XCTAssertTrue(mayDisturb(week: week, day: 3, minute: 60), "past midnight: early")
        XCTAssertFalse(mayDisturb(week: week, day: 3, minute: 720))
        XCTAssertTrue(mayDisturb(week: "garbage", day: 0, minute: 0), "what cannot be read never silences")
        XCTAssertTrue(mayDisturb(week: "x-y;all", day: 0, minute: 0))
    }

    func testTheHoursFollowThePhonesClockAndZone() {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "Europe/Madrid")!
        let monday = Date(timeIntervalSince1970: 1_791_181_800)
        let week = "510-1080;all;all;all;all;all;none"
        XCTAssertTrue(mayDisturb(week: week, at: monday, calendar: calendar))
        calendar.timeZone = TimeZone(identifier: "UTC")!
        XCTAssertFalse(mayDisturb(week: week, at: monday, calendar: calendar), "06:30 is before 08:30")
        let sunday = Date(timeIntervalSince1970: 1_791_100_800 + 12 * 3600)
        XCTAssertFalse(mayDisturb(week: week, at: sunday, calendar: calendar))
    }

    func testTheHoursOutliveTheApp() {
        let defaults = UserDefaults(suiteName: "ft.tests.quiet")!
        defaults.removePersistentDomain(forName: "ft.tests.quiet")
        XCTAssertEqual(QuietHours.week(in: defaults), "", "none kept: always")
        QuietHours.keep("all;all;all;all;all;none;none", in: defaults)
        XCTAssertEqual(QuietHours.week(in: defaults), "all;all;all;all;all;none;none")
        QuietHours.keep("", in: defaults)
        XCTAssertEqual(QuietHours.week(in: defaults), "")
    }

    func testTheOpenSlotsOutliveTheApp() {
        let defaults = UserDefaults(suiteName: "ft.tests.slots")!
        defaults.removePersistentDomain(forName: "ft.tests.slots")
        XCTAssertEqual(OpenSlots.kept(in: defaults), [], "none kept: none open")
        OpenSlots.keep([3, 1, 7], in: defaults)
        XCTAssertEqual(OpenSlots.kept(in: defaults), [1, 3, 7])
        OpenSlots.keep([0, 2, 8], in: defaults)
        XCTAssertEqual(OpenSlots.kept(in: defaults), [2], "only hidden sessions' slots")
        OpenSlots.keep([], in: defaults)
        XCTAssertEqual(OpenSlots.kept(in: defaults), [])
    }

    func testTheHoursAndTheOpenSlotsAreReadAsRustSendsThem() throws {
        let hours = try JSONDecoder().decode(QuietHoursArgs.self, from: Data(#"{"week":"all;none;all;all;all;all;all"}"#.utf8))
        XCTAssertEqual(hours.week, "all;none;all;all;all;all;all")
        let slots = try JSONDecoder().decode(OpenSlotsArgs.self, from: Data(#"{"slots":[1,3]}"#.utf8))
        XCTAssertEqual(slots.slots, [1, 3])
    }

    func testOnlyAnOpenSessionsSlotIsHeard() {
        XCTAssertTrue(slotIsHeard(nil, open: []))
        XCTAssertTrue(slotIsHeard(0, open: []))
        XCTAssertTrue(slotIsHeard(NSNumber(value: 0), open: []))
        XCTAssertFalse(slotIsHeard(3, open: []), "closed")
        XCTAssertTrue(slotIsHeard(3, open: [3]))
        XCTAssertTrue(slotIsHeard("3", open: [3]))
        XCTAssertFalse(slotIsHeard("x", open: [3]))
        XCTAssertFalse(slotIsHeard(2, open: [3]))
    }

    func testACallPushRingsOnlyWhenItMay() {
        XCTAssertEqual(pushCall(callKitCall: false, slotHeard: true, mayDisturb: true), .ring)
        XCTAssertEqual(pushCall(callKitCall: false, slotHeard: false, mayDisturb: true), .endAtOnce(.closedSession))
        XCTAssertEqual(pushCall(callKitCall: false, slotHeard: true, mayDisturb: false), .endAtOnce(.outsideHours))
        XCTAssertEqual(pushCall(callKitCall: false, slotHeard: false, mayDisturb: false), .endAtOnce(.closedSession))
        XCTAssertEqual(pushCall(callKitCall: true, slotHeard: true, mayDisturb: true), .alreadyRinging)
    }

    func testAQuietCallIsSeenButNeverRung() {
        XCTAssertTrue(quietCall(muted: true, mayDisturb: true))
        XCTAssertTrue(quietCall(muted: false, mayDisturb: false))
        XCTAssertFalse(quietCall(muted: false, mayDisturb: true))
        XCTAssertEqual(ringWith(callKitCall: false, answered: false, quiet: true), .silent)
        XCTAssertEqual(ringWith(callKitCall: true, answered: false, quiet: true), .endAndSilent)
        XCTAssertEqual(ringWith(callKitCall: true, answered: true, quiet: true), .update)
        XCTAssertEqual(ringWith(callKitCall: false, answered: false, quiet: false), .report)
        XCTAssertEqual(ringWith(callKitCall: true, answered: false, quiet: false), .update)
    }

    func testAQuietCallIsANotificationWithNoSoundOnlyAwayFromTheApp() {
        XCTAssertFalse(showsQuietNotice(appActive: true))
        XCTAssertTrue(showsQuietNotice(appActive: false))
        let content = quietCallContent(caller: "Alice", video: false)
        XCTAssertNil(content.sound)
        XCTAssertNil(content.badge)
        XCTAssertEqual(content.title, "Alice")
        XCTAssertEqual(content.body, NSLocalizedString("FT_INCOMING_CALL", comment: ""))
        XCTAssertEqual(quietCallContent(caller: "  ", video: true).title, "", "nameless: no title")
        XCTAssertEqual(quietCallContent(caller: "", video: true).body, NSLocalizedString("FT_INCOMING_VIDEO_CALL", comment: ""))
        XCTAssertFalse(quietCallIdentifier.hasPrefix("ft.reminder|"), "a tap is not taken for a reminder")
    }

    func testARefusedCallEndsOnlyThePushsUnnamedCall() {
        XCTAssertTrue(refusalEndsCall(callKitCall: true, outgoing: false, named: false))
        XCTAssertFalse(refusalEndsCall(callKitCall: true, outgoing: false, named: true))
        XCTAssertFalse(refusalEndsCall(callKitCall: true, outgoing: true, named: false))
        XCTAssertFalse(refusalEndsCall(callKitCall: false, outgoing: false, named: false))
    }

    func testACallEndedQuietlyLeavesNoMissedCall() {
        XCTAssertEqual(noTraceEndReason, .declinedElsewhere)
        XCTAssertNotEqual(noTraceEndReason, endReason(live: false))
    }

    func testAQuietCallAnsweredInTheAppJoinsCallKit() {
        XCTAssertEqual(appAnswer(callKitCall: true, answered: false, outgoing: false, quietRinging: false), .callKit)
        XCTAssertEqual(appAnswer(callKitCall: false, answered: false, outgoing: false, quietRinging: true), .join)
        XCTAssertEqual(appAnswer(callKitCall: false, answered: false, outgoing: false, quietRinging: false), .coreAlone)
        XCTAssertEqual(appAnswer(callKitCall: true, answered: true, outgoing: false, quietRinging: false), .coreAlone)
    }
}
