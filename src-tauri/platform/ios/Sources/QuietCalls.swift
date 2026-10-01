import CallKit
import Foundation
import UserNotifications

// Calls that must not ring on the iPhone (2026-10-01, Plan §108, §109): a closed hidden session's
// calls never show; a muted contact's, and any outside the weekly hours, show without ringing or
// vibrating; the ones the core refuses without a trace (Calls off, a stranger, a blocked contact)
// leave nothing. CallKit cannot ring in silence, so a quiet call never goes through it while it
// rings. The decisions are pure and tested (`QuietCalls_test.swift`); Android makes the same ones
// in PlatformPlugin.kt (`callPush`, `mayDisturb`, `wakeIsHeard`, `ringingFor`).

// ---- The weekly hours (app#7) ----

/// Whether the phone may make noise under the weekly hours. `week` is what the core hands over:
/// seven days, Monday first, separated by `;`, each `all`, `none` or `FROM-TO` in minutes of the
/// day (past midnight when TO comes first). Empty means no hours: always. Kotlin's `mayDisturb`.
func mayDisturb(week: String, day: Int, minute: Int) -> Bool {
    if week.isEmpty { return true }
    let days = week.split(separator: ";", omittingEmptySubsequences: false).map(String.init)
    guard days.indices.contains(day) else { return true }
    switch days[day] {
    case "all": return true
    case "none": return false
    case let span:
        let ends = span.split(separator: "-").compactMap { Int($0) }
        guard ends.count == 2 else { return true }
        let (from, to) = (ends[0], ends[1])
        return from <= to ? (from..<to).contains(minute) : minute >= from || minute < to
    }
}

/// The same, at `date` on the phone's clock and in its time zone (`calendar`).
func mayDisturb(week: String, at date: Date, calendar: Calendar) -> Bool {
    let parts = calendar.dateComponents([.weekday, .hour, .minute], from: date)
    // `weekday` counts from Sunday (1); the core's week starts on Monday.
    let monday = ((parts.weekday ?? 2) + 5) % 7
    return mayDisturb(week: week, day: monday, minute: (parts.hour ?? 0) * 60 + (parts.minute ?? 0))
}

/// The weekly hours as the core last handed them over, kept where a call push finds them when
/// PushKit launches the app before the core has started.
enum QuietHours {
    private static let key = "ft.quiet_hours"

    static func keep(_ week: String, in defaults: UserDefaults = .standard) {
        defaults.set(week, forKey: key)
    }

    static func week(in defaults: UserDefaults = .standard) -> String {
        defaults.string(forKey: key) ?? ""
    }

    /// Now, on the phone's clock and in its zone.
    static func mayDisturbNow() -> Bool {
        mayDisturb(week: week(), at: Date(), calendar: .current)
    }
}

/// Arguments of `setQuietHours`, the same as Kotlin's.
struct QuietHoursArgs: Decodable {
    let week: String
}

/// The hidden sessions open now, by slot, as the core last said (2026-10-01): kept where a call
/// push finds them when PushKit launches the app before the core. Never which session.
enum OpenSlots {
    private static let key = "ft.open_slots"

    static func keep(_ slots: [Int], in defaults: UserDefaults = .standard) {
        defaults.set(slots.filter { (1...7).contains($0) }.sorted(), forKey: key)
    }

    static func kept(in defaults: UserDefaults = .standard) -> Set<Int> {
        Set((defaults.array(forKey: key) as? [Int] ?? []).filter { (1...7).contains($0) })
    }
}

/// Arguments of `setOpenSlots`, the same as Kotlin's: the open hidden sessions, by slot.
struct OpenSlotsArgs: Decodable {
    let slots: [Int]
}

// ---- The call push (PushKit) ----

/// Whether a push for slot `raw` (the router's `s`, app#9) may make the phone ring: 0, or none
/// from an older router, is the main list; 1–7 are hidden sessions, heard only while open. A
/// process that just started has none open. Kotlin's `wakeIsHeard`.
func slotIsHeard(_ raw: Any?, open: Set<Int>) -> Bool {
    let slot: Int?
    switch raw {
    case nil: return true
    case let number as NSNumber: slot = number.intValue
    case let text as String: slot = Int(text)
    default: slot = nil
    }
    guard let slot else { return false }
    return slot == 0 || open.contains(slot)
}

/// Why a call push must not ring.
enum PushQuiet: Equatable {
    /// A hidden session that is not open on this phone (§108): nothing may show.
    case closedSession
    /// Outside the weekly hours (§109): the core shows the call quietly once it knows who calls.
    case outsideHours
}

/// What a call push does.
enum PushCall: Equatable {
    /// CallKit rings, as "FlickerTalk" until the core says who calls.
    case ring
    /// A CallKit call is there already: only the core is told to reconnect.
    case alreadyRinging
    /// Apple wants every VoIP push reported to CallKit: it is, and ended in the same turn, before
    /// it can ring.
    case endAtOnce(PushQuiet)
}

extension PushCall {
    /// For the device check's log: what was decided, never who or which session.
    var logName: String {
        switch self {
        case .ring: return "ring"
        case .alreadyRinging: return "already ringing"
        case .endAtOnce(.closedSession): return "ended at once (closed session)"
        case .endAtOnce(.outsideHours): return "ended at once (outside hours)"
        }
    }
}

func pushCall(callKitCall: Bool, slotHeard: Bool, mayDisturb: Bool) -> PushCall {
    if callKitCall { return .alreadyRinging }
    if !slotHeard { return .endAtOnce(.closedSession) }
    if !mayDisturb { return .endAtOnce(.outsideHours) }
    return .ring
}

// ---- A quiet call (app#4, app#7) ----

/// Whether a call the core rings must stay quiet: a muted contact, or outside the hours.
func quietCall(muted: Bool, mayDisturb: Bool) -> Bool { muted || !mayDisturb }

/// Whether a quiet call shows as a notification: only away from the app, which shows its own
/// incoming-call card on the screen.
func showsQuietNotice(appActive: Bool) -> Bool { !appActive }

/// The notification of a quiet call: one at a time, replaced by the next and taken away when the
/// call stops ringing (`Calls.stopQuiet`).
let quietCallIdentifier = "ft.call.quiet"

/// Who calls (no title when nameless) and the system's own words for a call; no sound, so no
/// vibration, and no badge. A tap opens the app on the call.
func quietCallContent(caller: String, video: Bool) -> UNMutableNotificationContent {
    let content = UNMutableNotificationContent()
    content.title = caller.trimmingCharacters(in: .whitespacesAndNewlines)
    content.body = NSLocalizedString(video ? "FT_INCOMING_VIDEO_CALL" : "FT_INCOMING_CALL", comment: "")
    content.sound = nil
    content.badge = nil
    content.categoryIdentifier = quietCallIdentifier
    return content
}

// ---- A call the core refused ----

/// Whether the core's refusal ends the CallKit call: only one a push reported that the core has
/// not named (rung or answered), incoming. A call the core rings, or this phone's own, is not the
/// refused one.
func refusalEndsCall(callKitCall: Bool, outgoing: Bool, named: Bool) -> Bool {
    callKitCall && !outgoing && !named
}

/// How a call that must leave no trace ends in CallKit: handled elsewhere, never unanswered (a
/// missed call). The calls are never in the phone's Recents either (`callProviderConfiguration`).
let noTraceEndReason: CXCallEndedReason = .declinedElsewhere

// ---- The app's answer button ----

/// How the app's answer button answers.
enum AppAnswer: Equatable {
    /// CallKit's own call: a `CXAnswerCallAction`, and the system activates the audio session.
    case callKit
    /// A quiet call, which CallKit never rang: it joins CallKit now (a started call), so the
    /// system activates the audio session the voice waits for; the core answers by itself.
    case join
    /// The core answers by itself.
    case coreAlone
}

func appAnswer(callKitCall: Bool, answered: Bool, outgoing: Bool, quietRinging: Bool) -> AppAnswer {
    if answerThroughCallKit(callKitCall: callKitCall, answered: answered, outgoing: outgoing) { return .callKit }
    return !callKitCall && quietRinging ? .join : .coreAlone
}

// ---- What the device check reads (debug builds only) ----

#if DEBUG
import os

private let quietCallsLog = Logger(subsystem: "com.flickertalk.calls", category: "quiet")
#endif

/// One line per decision, for checking on a device (`log stream --predicate 'subsystem ==
/// "com.flickertalk.calls"'`): only the decision's name, never who calls or which session. Not
/// in release builds.
func quietLog(_ decision: String) {
    #if DEBUG
    quietCallsLog.notice("ft-quiet \(decision, privacy: .public)")
    #endif
}
