import UserNotifications
import XCTest
@testable import tauri_plugin_ft_platform

final class RemindersTests: XCTestCase {
    func testRemindersAreReadAndCappedAtSixtyFour() throws {
        let entries = parseReminders(#"[{"plugin":"com.example.notes","id":"r1","at":5000,"text":"milk"},{"plugin":"","id":"x","at":1}]"#)
        XCTAssertEqual(entries.count, 1)
        XCTAssertEqual(reminderIdentifier(entries[0]), "ft.reminder|com.example.notes|r1")
        XCTAssertEqual(reminderKey("ft.reminder|com.example.notes|r1"), "com.example.notes\nr1")
        XCTAssertNil(reminderKey("something else"))
        XCTAssertEqual(parseReminders("junk").count, 0)
        var many: [ReminderEntry] = []
        for n in 0..<100 { many.append(ReminderEntry(plugin: "p", id: "r\(n)", at: Int64(1000 + (100 - n)), text: nil)) }
        many.append(ReminderEntry(plugin: "p", id: "past", at: 10, text: nil))
        let scheduled = remindersToSchedule(many, now: 500)
        XCTAssertEqual(scheduled.count, 64)
        XCTAssertEqual(scheduled.first?.id, "r99", "the soonest first")
        XCTAssertFalse(scheduled.contains { $0.id == "past" })
    }

    func testATappedNotificationIsAnsweredOnTheMainThread() {
        let taps = ReminderTaps()
        let answered = expectation(description: "answered")
        DispatchQueue.global().async {
            taps.tapped(identifier: "ft.reminder|com.example.notes|r1") {
                XCTAssertTrue(Thread.isMainThread)
                XCTAssertEqual(taps.pending, "com.example.notes\nr1")
                answered.fulfill()
            }
        }
        wait(for: [answered], timeout: 2)
    }

    func testANotificationOnTheScreenIsAnsweredOnTheMainThread() {
        let taps = ReminderTaps()
        let wake = expectation(description: "wake")
        let other = expectation(description: "other")
        DispatchQueue.global().async {
            taps.presenting(userInfo: ["t": "wake"]) { options in
                XCTAssertTrue(Thread.isMainThread)
                XCTAssertEqual(options, [], "the app on the screen already connects")
                wake.fulfill()
            }
            taps.presenting(userInfo: [:]) { options in
                XCTAssertTrue(Thread.isMainThread)
                XCTAssertEqual(options, [.banner, .sound, .list])
                other.fulfill()
            }
        }
        wait(for: [wake, other], timeout: 2)
    }
}
