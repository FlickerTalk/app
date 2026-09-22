import Foundation
import Security
import SwiftRs
import Tauri
import UIKit
import WebKit

// FlickerTalk's native bridge on iOS (see ../../src/lib.rs). For now only the storage key: it
// lives in the Keychain, on this device only (Plan §94). The rest of the bridge answers that it is
// not available on iOS yet, and the app carries on without it.

private let keyService = "com.flickertalk.app.storage"
private let keyAccount = "storage-key"

/// What Rust keeps in `storage.key.sealed` on iOS: only the name of the Keychain item.
func keychainMarker() -> Data {
    Data("keychain:v1".utf8)
}

/// Readable after the first unlock, and never synced to iCloud or restored on another device.
func keyAccessibility() -> String {
    kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly as String
}

class KeyArgs: Decodable {
    let value: String
}

enum KeychainError: Error {
    case status(OSStatus)
    case notFound
}

private func keyQuery() -> [String: Any] {
    [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: keyService,
        kSecAttrAccount as String: keyAccount,
    ]
}

private func storeKey(_ key: Data) throws {
    SecItemDelete(keyQuery() as CFDictionary)
    var item = keyQuery()
    item[kSecValueData as String] = key
    item[kSecAttrAccessible as String] = keyAccessibility()
    let status = SecItemAdd(item as CFDictionary, nil)
    guard status == errSecSuccess else { throw KeychainError.status(status) }
}

private func loadKey() throws -> Data {
    var query = keyQuery()
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne
    var result: AnyObject?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    guard status == errSecSuccess else { throw status == errSecItemNotFound ? KeychainError.notFound : KeychainError.status(status) }
    guard let key = result as? Data else { throw KeychainError.notFound }
    return key
}

class PlatformPlugin: Plugin {
    /// Keeps the storage key in the Keychain; Rust keeps only the marker.
    @objc public func sealKey(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(KeyArgs.self)
        guard let key = Data(base64Encoded: args.value), key.count == 32 else {
            invoke.reject("not a storage key")
            return
        }
        do {
            try storeKey(key)
            invoke.resolve(["value": keychainMarker().base64EncodedString()])
        } catch {
            invoke.reject("the Keychain refused the key: \(error)")
        }
    }

    @objc public func openKey(_ invoke: Invoke) throws {
        do {
            invoke.resolve(["value": try loadKey().base64EncodedString()])
        } catch {
            invoke.reject("the Keychain has no storage key: \(error)")
        }
    }
}

@_cdecl("init_plugin_ft_platform")
func initPlugin() -> Plugin {
    return PlatformPlugin()
}
