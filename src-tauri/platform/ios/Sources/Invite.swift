import Contacts
import ContactsUI
import Foundation
import Tauri
import UIKit

struct WhatsAppArgs: Decodable {
    let url: String
}

let nobodyPicked: [String: Any] = ["picked": false]

let contactsWithANumber = NSPredicate(format: "phoneNumbers.@count > 0")

let contactsWithOneNumber = NSPredicate(format: "phoneNumbers.@count == 1")

func phoneCountry(region: String?) -> String {
    (region ?? "").trimmingCharacters(in: .whitespaces).uppercased()
}

// iOS 16.4 stopped telling apps the SIM's country, so the region set on the phone stands in for it
func regionOfThePhone() -> String? {
    if #available(iOS 16, *) {
        return Locale.current.region?.identifier
    }
    return Locale.current.regionCode
}

func contactName(given: String, full: String?) -> String {
    let first = given.trimmingCharacters(in: .whitespacesAndNewlines)
    return first.isEmpty ? (full ?? "").trimmingCharacters(in: .whitespacesAndNewlines) : first
}

func pickedContactPayload(name: String, number: String, country: String) -> [String: Any] {
    ["picked": true, "name": name, "number": number, "country": country]
}

// the picker runs outside the app and shares only what was tapped, so no contacts permission is needed; nothing is kept
final class ContactPick: NSObject, CNContactPickerDelegate {
    private static var running: ContactPick?

    private let done: ([String: Any]) -> Void

    private init(done: @escaping ([String: Any]) -> Void) {
        self.done = done
        super.init()
    }

    static func present(on screen: UIViewController, done: @escaping ([String: Any]) -> Void) {
        let pick = ContactPick(done: done)
        running = pick
        let picker = CNContactPickerViewController()
        picker.delegate = pick
        picker.displayedPropertyKeys = [CNContactPhoneNumbersKey]
        picker.predicateForEnablingContact = contactsWithANumber
        picker.predicateForSelectionOfContact = contactsWithOneNumber
        screen.present(picker, animated: true)
    }

    func contactPicker(_ picker: CNContactPickerViewController, didSelect contact: CNContact) {
        let numbers = contact.isKeyAvailable(CNContactPhoneNumbersKey) ? contact.phoneNumbers : []
        finish(contact, number: numbers.first?.value.stringValue ?? "")
    }

    func contactPicker(_ picker: CNContactPickerViewController, didSelect contactProperty: CNContactProperty) {
        finish(contactProperty.contact, number: (contactProperty.value as? CNPhoneNumber)?.stringValue ?? "")
    }

    func contactPickerDidCancel(_ picker: CNContactPickerViewController) {
        done(nobodyPicked)
        ContactPick.running = nil
    }

    // reading a key the picker didn't fetch throws an Objective-C exception, so each one is checked first
    private func finish(_ contact: CNContact, number: String) {
        let given = contact.isKeyAvailable(CNContactGivenNameKey) ? contact.givenName : ""
        let formatted = contact.areKeysAvailable([CNContactFormatter.descriptorForRequiredKeys(for: .fullName)])
        let full = formatted ? CNContactFormatter.string(from: contact, style: .fullName) : nil
        done(pickedContactPayload(name: contactName(given: given, full: full), number: number, country: phoneCountry(region: regionOfThePhone())))
        ContactPick.running = nil
    }
}

extension PlatformPlugin {
    @objc public func pickContact(_ invoke: Invoke) throws {
        DispatchQueue.main.async { [manager] in
            guard let screen = manager.viewController else {
                invoke.reject("no screen to pick from")
                return
            }
            ContactPick.present(on: screen) { payload in invoke.resolve(payload.mapValues { $0 as Any? }) }
        }
    }

    // universal links only, so the chat opens in WhatsApp or not at all, never in Safari
    @objc public func openWhatsApp(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(WhatsAppArgs.self)
        guard let url = URL(string: args.url) else {
            invoke.resolve(["opened": false])
            return
        }
        DispatchQueue.main.async {
            UIApplication.shared.open(url, options: [.universalLinksOnly: true]) { opened in
                invoke.resolve(["opened": opened])
            }
        }
    }
}
