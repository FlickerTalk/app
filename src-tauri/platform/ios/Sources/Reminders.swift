import Foundation
import Tauri
import UserNotifications

struct ReminderEntry: Decodable, Equatable {
    let plugin: String
    let id: String
    let at: Int64
    let text: String?
}

func parseReminders(_ json: String) -> [ReminderEntry] {
    guard let data = json.data(using: .utf8),
          let entries = try? JSONDecoder().decode([ReminderEntry].self, from: data) else { return [] }
    return entries.filter { !$0.plugin.isEmpty && !$0.id.isEmpty && $0.at > 0 }
}

func reminderIdentifier(_ entry: ReminderEntry) -> String { "ft.reminder|" + entry.plugin + "|" + entry.id }

func reminderKey(_ identifier: String) -> String? {
    let parts = identifier.split(separator: "|", maxSplits: 2).map(String.init)
    guard parts.count == 3, parts[0] == "ft.reminder" else { return nil }
    return parts[1] + "\n" + parts[2]
}

// iOS keeps at most 64 pending notifications, so set the soonest
func remindersToSchedule(_ entries: [ReminderEntry], now: Int64, limit: Int = 64) -> [ReminderEntry] {
    Array(entries.filter { $0.at > now }.sorted { $0.at < $1.at }.prefix(limit))
}

class RemindersArgs: Decodable {
    let reminders: String
}

final class ReminderTaps: NSObject, UNUserNotificationCenterDelegate {
    static let shared = ReminderTaps()
    var pending: String = ""

    // iOS may call this off the main thread and UIKit wants the answer on main, otherwise a tap killed the app
    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
        tapped(identifier: response.notification.request.identifier, done: completionHandler)
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        presenting(userInfo: notification.request.content.userInfo, done: completionHandler)
    }

    func tapped(identifier: String, done: @escaping () -> Void) {
        DispatchQueue.main.async {
            if let key = reminderKey(identifier) { self.pending = key }
            done()
        }
    }

    func presenting(userInfo: [AnyHashable: Any], done: @escaping (UNNotificationPresentationOptions) -> Void) {
        let options: UNNotificationPresentationOptions = isWakePush(userInfo) ? [] : [.banner, .sound, .list]
        DispatchQueue.main.async { done(options) }
    }
}

extension PlatformPlugin {
    @objc public func pendingReminder(_ invoke: Invoke) throws {
        invoke.resolve(["reminder": ReminderTaps.shared.pending])
        ReminderTaps.shared.pending = ""
    }

    // no note text unless the core sent one
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
}
