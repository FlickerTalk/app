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
}
