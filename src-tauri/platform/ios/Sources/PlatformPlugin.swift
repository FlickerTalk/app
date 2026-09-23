import Foundation
import Security
import StoreKit
import SwiftRs
import Tauri
import UIKit
import WebKit

// FlickerTalk's native bridge on iOS (see ../../src/lib.rs). For now the storage key, which lives
// in the Keychain on this device only (Plan §94), and the share sheet. The rest of the bridge
// answers that it is not available on iOS yet, and the app carries on without it.

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

/// The text for the share sheet, or nil when there is nothing to share.
func shareableText(_ text: String) -> String? {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}

/// The yearly subscription, as it is named in App Store Connect (§40-42).
let yearly = "yearly"

/// One entitlement as the Store handed it back, with none of StoreKit's types in it.
struct StoreEntitlement {
    let product: String
    let expires: Date?
    let revoked: Date?
}

/// Until when this phone is paid up, in milliseconds as the core counts time; 0 when nothing is.
/// StoreKit says when a subscription runs out, so unlike Play nothing is guessed here (§45).
func activeUntil(_ entitlements: [StoreEntitlement], now: Date) -> Int64 {
    entitlements
        .filter { $0.product == yearly && $0.revoked == nil }
        .compactMap { $0.expires }
        .filter { $0 > now }
        .map { Int64(($0.timeIntervalSince1970 * 1000).rounded()) }
        .max() ?? 0
}

class ShareArgs: Decodable {
    let text: String
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

    /// The system share sheet (WhatsApp, Signal, mail…) with a text, such as the card link (§32).
    @objc public func shareText(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(ShareArgs.self)
        guard let text = shareableText(args.text) else {
            invoke.reject("nothing to share")
            return
        }
        DispatchQueue.main.async { [manager] in
            guard let screen = manager.viewController else {
                invoke.reject("no screen to share from")
                return
            }
            let sheet = UIActivityViewController(activityItems: [text], applicationActivities: nil)
            // On an iPad the sheet is a popover: it needs an anchor.
            UIUtils.centerPopover(rootViewController: screen, popoverController: sheet)
            screen.present(sheet, animated: true)
            invoke.resolve()
        }
    }

    @objc public func openKey(_ invoke: Invoke) throws {
        do {
            invoke.resolve(["value": try loadKey().base64EncodedString()])
        } catch {
            invoke.reject("the Keychain has no storage key: \(error)")
        }
    }

    // What these commands reject with are keys, not sentences: the app turns them into the
    // user's own language (`plan.trouble.*`), so nothing raw ever reaches the screen.
    /// What the Store already knows about this phone, without asking anyone to buy anything.
    private func entitlements() async -> [StoreEntitlement] {
        var found: [StoreEntitlement] = []
        for await result in Transaction.currentEntitlements {
            guard case .verified(let transaction) = result else { continue }
            found.append(
                StoreEntitlement(
                    product: transaction.productID,
                    expires: transaction.expirationDate,
                    revoked: transaction.revocationDate
                )
            )
        }
        return found
    }

    /// What the Store knows already (§45): the app asks every time it opens, and nothing else.
    @objc public func subscription(_ invoke: Invoke) throws {
        Task {
            invoke.resolve(["until": activeUntil(await entitlements(), now: Date())])
        }
    }

    /// The yearly subscription (§45, §47). Apple holds the money and the card; FlickerTalk only
    /// learns until when this phone is paid up.
    @objc public func subscribe(_ invoke: Invoke) throws {
        Task {
            do {
                guard let product = try await Product.products(for: [yearly]).first else {
                    invoke.reject("not_on_sale")
                    return
                }
                switch try await product.purchase() {
                case .success(let signed):
                    guard case .verified(let transaction) = signed else {
                        invoke.reject("payment_failed")
                        return
                    }
                    await transaction.finish()
                    invoke.resolve(["until": activeUntil(await entitlements(), now: Date())])
                case .userCancelled:
                    invoke.reject("cancelled")
                case .pending:
                    // Ask to Buy and the like: not paid for, so it is not pretended to be (§84).
                    invoke.reject("pending_approval")
                @unknown default:
                    invoke.reject("payment_failed")
                }
            } catch {
                invoke.reject("payment_failed")
            }
        }
    }
}

@_cdecl("init_plugin_ft_platform")
func initPlugin() -> Plugin {
    return PlatformPlugin()
}
