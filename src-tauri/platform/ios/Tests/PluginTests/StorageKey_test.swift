import XCTest
@testable import tauri_plugin_ft_platform

final class StorageKeyTests: XCTestCase {
    func testTheSealedFormHoldsNoKey() throws {
        let key = Data(repeating: 7, count: 32)
        let marker = keychainMarker()
        XCTAssertFalse(marker.range(of: key) != nil)
        XCTAssertEqual(String(data: marker, encoding: .utf8), "keychain:v1")
    }

    func testTheKeyStaysOnThisDevice() throws {
        XCTAssertEqual(keyAccessibility(), kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly as String)
    }

    func testErasingForgetsTheKeyInTheKeychain() throws {
        do {
            try storeKey(Data(repeating: 9, count: 32))
        } catch KeychainError.status(let status) where status == errSecMissingEntitlement {
            throw XCTSkip("this test runner has no Keychain")
        }
        XCTAssertEqual(try loadKey(), Data(repeating: 9, count: 32))
        try forgetStoredKey()
        XCTAssertThrowsError(try loadKey())
        XCTAssertNoThrow(try forgetStoredKey())
    }
}
