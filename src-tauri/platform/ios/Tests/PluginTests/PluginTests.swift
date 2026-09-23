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
}
