import CallKit
import UserNotifications
import XCTest
@testable import tauri_plugin_ft_platform

// Calls that must not ring on the iPhone (2026-10-01, Plan §108, §109): a closed hidden session's,
// a muted contact's, outside the weekly hours, and the ones the core refuses without a trace.
// Android does the same in PlatformPlugin.kt (`callPush`, `mayDisturb`, `wakeIsHeard`).
final class QuietCallsTests: XCTestCase {
    // The weekly hours as the core hands them over (app#7): seven days, Monday first, each `all`,
    // `none` or `FROM-TO` in minutes of the day, past midnight when TO comes first. The same rule
    // as Kotlin's `mayDisturb`.
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

    // The phone's own clock and time zone: Monday is day 0.
    func testTheHoursFollowThePhonesClockAndZone() {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "Europe/Madrid")!
        // 2026-10-05 is a Monday; 06:30 UTC is 08:30 in Madrid.
        let monday = Date(timeIntervalSince1970: 1_791_181_800)
        let week = "510-1080;all;all;all;all;all;none"
        XCTAssertTrue(mayDisturb(week: week, at: monday, calendar: calendar))
        calendar.timeZone = TimeZone(identifier: "UTC")!
        XCTAssertFalse(mayDisturb(week: week, at: monday, calendar: calendar), "06:30 is before 08:30")
        // Sunday is the last day of the week.
        let sunday = Date(timeIntervalSince1970: 1_791_100_800 + 12 * 3600)
        XCTAssertFalse(mayDisturb(week: week, at: sunday, calendar: calendar))
    }

    // The hours are kept where a call push finds them with the app just launched (PushKit).
    func testTheHoursOutliveTheApp() {
        let defaults = UserDefaults(suiteName: "ft.tests.quiet")!
        defaults.removePersistentDomain(forName: "ft.tests.quiet")
        XCTAssertEqual(QuietHours.week(in: defaults), "", "none kept: always")
        QuietHours.keep("all;all;all;all;all;none;none", in: defaults)
        XCTAssertEqual(QuietHours.week(in: defaults), "all;all;all;all;all;none;none")
        QuietHours.keep("", in: defaults)
        XCTAssertEqual(QuietHours.week(in: defaults), "")
    }

    // What Rust sends (`QuietHours`, `OpenSlots` in platform/src/lib.rs).
    func testTheHoursAndTheOpenSlotsAreReadAsRustSendsThem() throws {
        let hours = try JSONDecoder().decode(QuietHoursArgs.self, from: Data(#"{"week":"all;none;all;all;all;all;all"}"#.utf8))
        XCTAssertEqual(hours.week, "all;none;all;all;all;all;all")
        let slots = try JSONDecoder().decode(OpenSlotsArgs.self, from: Data(#"{"slots":[1,3]}"#.utf8))
        XCTAssertEqual(slots.slots, [1, 3])
    }

    // The router's `s` (app#9): 0, or none from an older router, is the main list; 1–7 a hidden
    // session, heard only while it is open. After a process start none is open. APNs carries a
    // number; a string is read too, and anything else is not heard.
    func testOnlyAnOpenSessionsSlotIsHeard() {
        XCTAssertTrue(slotIsHeard(nil, open: []))
        XCTAssertTrue(slotIsHeard(0, open: []))
        XCTAssertTrue(slotIsHeard(NSNumber(value: 0), open: []))
        XCTAssertFalse(slotIsHeard(3, open: []), "closed: a process that just started has none open")
        XCTAssertTrue(slotIsHeard(3, open: [3]))
        XCTAssertTrue(slotIsHeard("3", open: [3]))
        XCTAssertFalse(slotIsHeard("x", open: [3]))
        XCTAssertFalse(slotIsHeard(2, open: [3]))
    }

    // Apple wants every VoIP push reported to CallKit. One for a closed session, or outside the
    // hours, is reported and ended in the same turn, before it rings: the core, which reconnects,
    // answers busy for the closed session, or shows the call quietly once it knows who calls.
    func testACallPushRingsOnlyWhenItMay() {
        XCTAssertEqual(pushCall(callKitCall: false, slotHeard: true, mayDisturb: true), .ring)
        XCTAssertEqual(pushCall(callKitCall: false, slotHeard: false, mayDisturb: true), .endAtOnce(.closedSession))
        XCTAssertEqual(pushCall(callKitCall: false, slotHeard: true, mayDisturb: false), .endAtOnce(.outsideHours))
        XCTAssertEqual(pushCall(callKitCall: false, slotHeard: false, mayDisturb: false), .endAtOnce(.closedSession))
        XCTAssertEqual(pushCall(callKitCall: true, slotHeard: true, mayDisturb: true), .alreadyRinging)
    }

    // A muted contact (app#4) or outside the hours (app#7): the call shows, never through
    // CallKit, which cannot ring in silence. A CallKit call PushKit reported for it ends first.
    // An answered call is the user's already: it only learns who it is.
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

    // Shown quietly: the app's own incoming-call card when the app is on the screen; else a
    // notification with no sound (so no vibration either) and no badge, which opens the app.
    func testAQuietCallIsANotificationWithNoSoundOnlyAwayFromTheApp() {
        XCTAssertFalse(showsQuietNotice(appActive: true))
        XCTAssertTrue(showsQuietNotice(appActive: false))
        let content = quietCallContent(caller: "Ioan", video: false)
        XCTAssertNil(content.sound)
        XCTAssertNil(content.badge)
        XCTAssertEqual(content.title, "Ioan")
        XCTAssertEqual(content.body, NSLocalizedString("FT_INCOMING_CALL", comment: ""))
        XCTAssertEqual(quietCallContent(caller: "  ", video: true).title, "", "nameless: no title")
        XCTAssertEqual(quietCallContent(caller: "", video: true).body, NSLocalizedString("FT_INCOMING_VIDEO_CALL", comment: ""))
        XCTAssertFalse(quietCallIdentifier.hasPrefix("ft.reminder|"), "a tap is not taken for a reminder")
    }

    // The core refused the call without a trace (Calls off, a stranger, a blocked contact, a
    // closed session): the CallKit call a push reported, which the core has not named, ends. A
    // call the core rings, this phone's own call, and nothing at all are left alone.
    func testARefusedCallEndsOnlyThePushsUnnamedCall() {
        XCTAssertTrue(refusalEndsCall(callKitCall: true, outgoing: false, named: false))
        XCTAssertFalse(refusalEndsCall(callKitCall: true, outgoing: false, named: true))
        XCTAssertFalse(refusalEndsCall(callKitCall: true, outgoing: true, named: false))
        XCTAssertFalse(refusalEndsCall(callKitCall: false, outgoing: false, named: false))
    }

    // Ended so that nothing says it rang: not "unanswered" (a missed call), but handled elsewhere.
    func testACallEndedQuietlyLeavesNoMissedCall() {
        XCTAssertEqual(noTraceEndReason, .declinedElsewhere)
        XCTAssertNotEqual(noTraceEndReason, endReason(live: false))
    }

    // The app's answer: CallKit answers its own call; a quiet call (no CallKit call) joins
    // CallKit as the user answers, so the system activates the audio session the voice waits
    // for; the core answers it by itself. With neither, the core answers alone.
    func testAQuietCallAnsweredInTheAppJoinsCallKit() {
        XCTAssertEqual(appAnswer(callKitCall: true, answered: false, outgoing: false, quietRinging: false), .callKit)
        XCTAssertEqual(appAnswer(callKitCall: false, answered: false, outgoing: false, quietRinging: true), .join)
        XCTAssertEqual(appAnswer(callKitCall: false, answered: false, outgoing: false, quietRinging: false), .coreAlone)
        XCTAssertEqual(appAnswer(callKitCall: true, answered: true, outgoing: false, quietRinging: false), .coreAlone)
    }
}
