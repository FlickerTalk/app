import AuthenticationServices
import Foundation
import QuickLook
import Security
import StoreKit
import SwiftRs
import Tauri
import UIKit
import UserNotifications
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

/// One reminder as the core wrote it (2026-09-27).
struct ReminderEntry: Decodable, Equatable {
    let plugin: String
    let id: String
    let at: Int64
    let text: String?
}

/// What the core sends: `[{plugin, id, at, text}]`. Anything unreadable is nothing, not a crash.
func parseReminders(_ json: String) -> [ReminderEntry] {
    guard let data = json.data(using: .utf8),
          let entries = try? JSONDecoder().decode([ReminderEntry].self, from: data) else { return [] }
    return entries.filter { !$0.plugin.isEmpty && !$0.id.isEmpty && $0.at > 0 }
}

/// The notification's identifier: one per reminder, so the same reminder replaces its own.
func reminderIdentifier(_ entry: ReminderEntry) -> String { "ft.reminder|" + entry.plugin + "|" + entry.id }

/// `plugin\nid`, what the app is opened with when a reminder is tapped.
func reminderKey(_ identifier: String) -> String? {
    let parts = identifier.split(separator: "|", maxSplits: 2).map(String.init)
    guard parts.count == 3, parts[0] == "ft.reminder" else { return nil }
    return parts[1] + "\n" + parts[2]
}

/// iOS keeps at most 64 pending notifications per app: the soonest ones are set, the rest wait
/// for the next start.
func remindersToSchedule(_ entries: [ReminderEntry], now: Int64, limit: Int = 64) -> [ReminderEntry] {
    Array(entries.filter { $0.at > now }.sorted { $0.at < $1.at }.prefix(limit))
}

/// Whether a login's redirect is the one we wait for: our scheme, not some other link.
func isAuthRedirect(_ url: URL?, scheme: String) -> Bool {
    guard let url, !scheme.isEmpty else { return false }
    return url.scheme?.lowercased() == scheme.lowercased()
}

class RemindersArgs: Decodable {
    let reminders: String
}

class AuthorizeArgs: Decodable {
    let url: String
    let scheme: String
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

/// Hears the tap on a reminder notification and keeps it until the app asks (2026-09-27).
final class ReminderTaps: NSObject, UNUserNotificationCenterDelegate {
    static let shared = ReminderTaps()
    var pending: String = ""

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        if let key = reminderKey(response.notification.request.identifier) { pending = key }
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        [.banner, .sound, .list]
    }
}

/// Arguments of `openFile`, the same as Kotlin's: a path inside the app and its kind.
struct OpenFileArgs: Decodable {
    let path: String
    let mime: String
}

/// The one item Quick Look shows: the file, as it is on this phone (document viewer, 2026-09-27).
final class PreviewItem: NSObject, QLPreviewControllerDataSource, QLPreviewItem {
    let previewItemURL: URL?
    let previewItemTitle: String?

    init(url: URL) {
        previewItemURL = url
        previewItemTitle = url.lastPathComponent
    }

    func numberOfPreviewItems(in controller: QLPreviewController) -> Int { 1 }

    func previewController(_ controller: QLPreviewController, previewItemAt index: Int) -> QLPreviewItem { self }
}

/// The file a tap wants shown, if it is there and Quick Look can show it; nil otherwise. Pure,
/// so a test can check it without a screen.
func previewable(path: String) -> PreviewItem? {
    guard !path.isEmpty, FileManager.default.fileExists(atPath: path) else { return nil }
    let item = PreviewItem(url: URL(fileURLWithPath: path))
    return QLPreviewController.canPreview(item) ? item : nil
}

class PlatformPlugin: Plugin {
    /// A login sheet waiting for the provider to send the user back (drive, 2026-09-27).
    private var authSession: ASWebAuthenticationSession?
    private let authAnchor = AuthAnchor()

    override init() {
        super.init()
        UNUserNotificationCenter.current().delegate = ReminderTaps.shared
    }

    /// The reminder the user tapped to open the app, once (2026-09-27).
    @objc public func pendingReminder(_ invoke: Invoke) throws {
        invoke.resolve(["reminder": ReminderTaps.shared.pending])
        ReminderTaps.shared.pending = ""
    }

    /// Every reminder there is, from the core: what iOS had is replaced. The notification says
    /// only "you have a reminder" unless the core sent a text.
    @objc public func setReminders(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(RemindersArgs.self)
        let entries = parseReminders(args.reminders)
        let center = UNUserNotificationCenter.current()
        center.getPendingNotificationRequests { requests in
            let ours = requests.map(\.identifier).filter { $0.hasPrefix("ft.reminder|") }
            center.removePendingNotificationRequests(withIdentifiers: ours)
            let now = Int64(Date().timeIntervalSince1970 * 1000)
            for entry in remindersToSchedule(entries, now: now) {
                let content = UNMutableNotificationContent()
                content.title = NSLocalizedString("Reminder", comment: "")
                content.body = (entry.text ?? "").isEmpty ? NSLocalizedString("You have a reminder", comment: "") : entry.text!
                content.sound = .default
                content.categoryIdentifier = "ft.reminder"
                let date = Date(timeIntervalSince1970: TimeInterval(entry.at) / 1000)
                let parts = Calendar.current.dateComponents([.year, .month, .day, .hour, .minute, .second], from: date)
                let trigger = UNCalendarNotificationTrigger(dateMatching: parts, repeats: false)
                center.add(UNNotificationRequest(identifier: reminderIdentifier(entry), content: content, trigger: trigger))
            }
            invoke.resolve()
        }
    }

    /// A login in the system's sheet (drive, 2026-09-27): `ASWebAuthenticationSession` shows
    /// the provider's page and hands back the URL with our scheme. The WebView sees none of it.
    @objc public func authorize(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(AuthorizeArgs.self)
        guard let url = URL(string: args.url) else {
            invoke.reject("not a login page")
            return
        }
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            let session = ASWebAuthenticationSession(url: url, callbackURLScheme: args.scheme) { callback, error in
                if let callback, isAuthRedirect(callback, scheme: args.scheme) {
                    invoke.resolve(["url": callback.absoluteString])
                } else {
                    invoke.reject(error?.localizedDescription ?? "cancelled")
                }
                self.authSession = nil
            }
            session.presentationContextProvider = self.authAnchor
            session.prefersEphemeralWebBrowserSession = false
            self.authSession = session
            session.start()
        }
    }

    /// Shows a file of the app inside the app, with the system's Quick Look (document viewer,
    /// 2026-09-27): PDF, Office, Pages, text and pictures. Its share button hands the file to
    /// another app, as Android's `openFile` does directly.
    @objc public func openFile(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(OpenFileArgs.self)
        guard let item = previewable(path: args.path) else {
            invoke.reject("that file cannot be shown here")
            return
        }
        DispatchQueue.main.async { [manager] in
            guard let screen = manager.viewController else {
                invoke.reject("no screen to show it on")
                return
            }
            let preview = QLPreviewController()
            preview.dataSource = item
            // The data source is held only weakly by the controller: keep it as long as it shows.
            objc_setAssociatedObject(preview, "ft.preview.item", item, .OBJC_ASSOCIATION_RETAIN)
            screen.present(preview, animated: true)
            invoke.resolve()
        }
    }

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

/// Where the login sheet is shown: the key window.
final class AuthAnchor: NSObject, ASWebAuthenticationPresentationContextProviding {
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows)
            .first { $0.isKeyWindow } ?? ASPresentationAnchor()
    }
}

@_cdecl("init_plugin_ft_platform")
func initPlugin() -> Plugin {
    return PlatformPlugin()
}
