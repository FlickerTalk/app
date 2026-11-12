import CallKit
import Foundation
import UserNotifications

// CallKit can't ring silently, so quiet calls stay out of it while they ring.

// week is seven ;-separated days from Monday, each all, none or FROM-TO in minutes of the day
// (past midnight when TO comes first). Empty means no quiet hours.
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

func mayDisturb(week: String, at date: Date, calendar: Calendar) -> Bool {
    let parts = calendar.dateComponents([.weekday, .hour, .minute], from: date)
    // weekday counts from Sunday = 1, the core's week starts on Monday
    let monday = ((parts.weekday ?? 2) + 5) % 7
    return mayDisturb(week: week, day: monday, minute: (parts.hour ?? 0) * 60 + (parts.minute ?? 0))
}

// stored, a call push may launch the app before the core is up
enum QuietHours {
    private static let key = "ft.quiet_hours"

    static func keep(_ week: String, in defaults: UserDefaults = .standard) {
        defaults.set(week, forKey: key)
    }

    static func week(in defaults: UserDefaults = .standard) -> String {
        defaults.string(forKey: key) ?? ""
    }

    static func mayDisturbNow() -> Bool {
        mayDisturb(week: week(), at: Date(), calendar: .current)
    }
}

struct QuietHoursArgs: Decodable {
    let week: String
}

// only slot numbers, never which session
enum OpenSlots {
    private static let key = "ft.open_slots"

    static func keep(_ slots: [Int], in defaults: UserDefaults = .standard) {
        defaults.set(slots.filter { (1...7).contains($0) }.sorted(), forKey: key)
    }

    static func kept(in defaults: UserDefaults = .standard) -> Set<Int> {
        Set((defaults.array(forKey: key) as? [Int] ?? []).filter { (1...7).contains($0) })
    }
}

struct OpenSlotsArgs: Decodable {
    let slots: [Int]
}

// slot 0 (or none, from an older router) is the main list, 1-7 are hidden sessions
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

enum PushQuiet: Equatable {
    case closedSession
    case outsideHours
}

enum PushCall: Equatable {
    case ring
    case alreadyRinging
    // Apple wants every VoIP push reported to CallKit, so report it and end it in the same turn
    case endAtOnce(PushQuiet)
}

extension PushCall {
    // never who calls or which session
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

func quietCall(muted: Bool, mayDisturb: Bool) -> Bool { muted || !mayDisturb }

func showsQuietNotice(appActive: Bool) -> Bool { !appActive }

let quietCallIdentifier = "ft.call.quiet"

// no sound means no vibration either
func quietCallContent(caller: String, video: Bool) -> UNMutableNotificationContent {
    let content = UNMutableNotificationContent()
    content.title = caller.trimmingCharacters(in: .whitespacesAndNewlines)
    content.body = NSLocalizedString(video ? "FT_INCOMING_VIDEO_CALL" : "FT_INCOMING_CALL", comment: "")
    content.sound = nil
    content.badge = nil
    content.categoryIdentifier = quietCallIdentifier
    return content
}

func refusalEndsCall(callKitCall: Bool, outgoing: Bool, named: Bool) -> Bool {
    callKitCall && !outgoing && !named
}

// .declinedElsewhere rather than .unanswered, so no missed call shows
let noTraceEndReason: CXCallEndedReason = .declinedElsewhere

enum AppAnswer: Equatable {
    case callKit
    // joins CallKit as a started call so the system activates the audio session
    case join
    case coreAlone
}

func appAnswer(callKitCall: Bool, answered: Bool, outgoing: Bool, quietRinging: Bool) -> AppAnswer {
    if answerThroughCallKit(callKitCall: callKitCall, answered: answered, outgoing: outgoing) { return .callKit }
    return !callKitCall && quietRinging ? .join : .coreAlone
}

#if DEBUG
import os

private let quietCallsLog = Logger(subsystem: "com.flickertalk.calls", category: "quiet")
#endif

// debug only, read with log stream --predicate 'subsystem == "com.flickertalk.calls"'. Never who or which session.
func quietLog(_ decision: String) {
    #if DEBUG
    quietCallsLog.notice("ft-quiet \(decision, privacy: .public)")
    #endif
}
