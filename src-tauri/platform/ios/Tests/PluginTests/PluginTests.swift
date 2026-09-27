import QuickLook
import XCTest
@testable import tauri_plugin_ft_platform

final class PlatformPluginTests: XCTestCase {
    // What Rust keeps in `storage.key.sealed` on iOS only names the Keychain item: the key itself
    // never leaves the Keychain.
    func testTheSealedFormHoldsNoKey() throws {
        let key = Data(repeating: 7, count: 32)
        let marker = keychainMarker()
        XCTAssertFalse(marker.range(of: key) != nil)
        XCTAssertEqual(String(data: marker, encoding: .utf8), "keychain:v1")
    }

    // The item is readable only on this device, after the first unlock (no iCloud, no backups).
    func testTheKeyStaysOnThisDevice() throws {
        XCTAssertEqual(keyAccessibility(), kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly as String)
    }

    // The share sheet gets the text without stray spaces, and never empty.
    func testOnlyRealTextIsShared() throws {
        XCTAssertEqual(shareableText("  Add me: https://flickertalk.com/add#card\n"), "Add me: https://flickertalk.com/add#card")
        XCTAssertNil(shareableText("   "))
    }

    // What the Store handed back about one entitlement, as the plugin reads it.
    private func entitlement(
        product: String = yearly,
        expires: Date? = Date(timeIntervalSince1970: 1_821_744_000),
        revoked: Date? = nil
    ) -> StoreEntitlement {
        StoreEntitlement(product: product, expires: expires, revoked: revoked)
    }

    // Unlike Play, StoreKit says until when the subscription runs, so the phone keeps that date
    // instead of guessing a year; the core counts time in milliseconds (§45).
    func testTheStoreOwnDateIsKept() throws {
        let runsOut = Date(timeIntervalSince1970: 1_821_744_000)
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        XCTAssertEqual(activeUntil([entitlement(expires: runsOut)], now: now), 1_821_744_000_000)
    }

    func testNothingBoughtIsNoSubscription() throws {
        XCTAssertEqual(activeUntil([], now: Date(timeIntervalSince1970: 1_790_208_000)), 0)
    }

    // Whatever else the Apple ID carries, only our own product pays for FlickerTalk.
    func testOnlyTheYearlyProductCounts() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        XCTAssertEqual(activeUntil([entitlement(product: "com.someone.else.pro")], now: now), 0)
    }

    // Apple gives money back, and says so: a refunded purchase stops paying at once (§84).
    func testARefundedPurchaseStopsCounting() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        let refunded = entitlement(revoked: Date(timeIntervalSince1970: 1_790_100_000))
        XCTAssertEqual(activeUntil([refunded], now: now), 0)
    }

    // A subscription that ran out is not one, however it reached the phone.
    func testAnEntitlementThatRanOutIsNotASubscription() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        let old = entitlement(expires: Date(timeIntervalSince1970: 1_790_100_000))
        XCTAssertEqual(activeUntil([old], now: now), 0)
    }

    // A renewal arrives as a later date: the phone follows the furthest one.
    func testTheFurthestEntitlementCounts() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        let soon = entitlement(expires: Date(timeIntervalSince1970: 1_800_000_000))
        let later = entitlement(expires: Date(timeIntervalSince1970: 1_821_744_000))
        XCTAssertEqual(activeUntil([soon, later], now: now), 1_821_744_000_000)
    }

    // Local reminders (2026-09-27): read leniently, one identifier per reminder, and never more
    // than the 64 iOS allows, the soonest first.
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

    // A login's redirect is only the one with our scheme.
    func testOnlyOurSchemeEndsALogin() throws {
        XCTAssertTrue(isAuthRedirect(URL(string: "com.flickertalk.app:/oauth?code=abc"), scheme: "com.flickertalk.app"))
        XCTAssertFalse(isAuthRedirect(URL(string: "https://evil.example/?code=abc"), scheme: "com.flickertalk.app"))
        XCTAssertFalse(isAuthRedirect(nil, scheme: "com.flickertalk.app"))
    }

    // Document viewer (2026-09-27): a tap shows the file with Quick Look, if it is there.
    func testOnlyAFileThatIsThereCanBePreviewed() throws {
        XCTAssertNil(previewable(path: ""))
        XCTAssertNil(previewable(path: "/nowhere/missing.pdf"))
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("ft-preview-\(UUID().uuidString).txt")
        try "hello".write(to: file, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: file) }
        let item = try XCTUnwrap(previewable(path: file.path))
        XCTAssertEqual(item.previewItemURL, file)
        XCTAssertEqual(item.previewItemTitle, file.lastPathComponent)
        XCTAssertEqual(item.numberOfPreviewItems(in: QLPreviewController()), 1)
    }
}
