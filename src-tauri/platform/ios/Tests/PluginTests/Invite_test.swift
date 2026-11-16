import Contacts
import XCTest
@testable import tauri_plugin_ft_platform

final class InviteTests: XCTestCase {
    private static let srcTauri = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
    private static let appResources = srcTauri.appendingPathComponent("gen/apple/flickertalk_iOS")

    private func contact(numbers: [String]) -> CNMutableContact {
        let contact = CNMutableContact()
        contact.givenName = "Ana"
        contact.phoneNumbers = numbers.map { CNLabeledValue(label: CNLabelPhoneNumberMobile, value: CNPhoneNumber(stringValue: $0)) }
        return contact
    }

    func testThePhoneCountryIsTheRegionSetOnThePhone() {
        XCTAssertEqual(phoneCountry(region: "es"), "ES")
        XCTAssertEqual(phoneCountry(region: " GB "), "GB")
        XCTAssertEqual(phoneCountry(region: nil), "")
    }

    func testAPickedContactGoesBackWithItsNameNumberAndThePhoneCountry() {
        XCTAssertEqual(
            pickedContactPayload(name: "Ana", number: "612 345 678", country: "ES") as NSDictionary,
            ["picked": true, "name": "Ana", "number": "612 345 678", "country": "ES"] as NSDictionary
        )
    }

    func testAClosedPickerGoesBackAsNobody() {
        XCTAssertEqual(nobodyPicked as NSDictionary, ["picked": false] as NSDictionary)
    }

    func testTheNameIsTheGivenNameOrElseTheWholeName() {
        XCTAssertEqual(contactName(given: "Ana", full: "Ana García"), "Ana")
        XCTAssertEqual(contactName(given: " ", full: " Acme Ltd "), "Acme Ltd")
        XCTAssertEqual(contactName(given: "", full: nil), "")
    }

    func testOnlyContactsWithANumberCanBePickedAndOneWithSeveralShowsThemToChooseFrom() {
        XCTAssertFalse(contactsWithANumber.evaluate(with: contact(numbers: [])))
        XCTAssertTrue(contactsWithANumber.evaluate(with: contact(numbers: ["612 345 678"])))
        XCTAssertTrue(contactsWithOneNumber.evaluate(with: contact(numbers: ["612 345 678"])))
        XCTAssertFalse(contactsWithOneNumber.evaluate(with: contact(numbers: ["612 345 678", "91 123 45 67"])))
    }

    func testThePlistsNeverAskForTheContacts() throws {
        for plist in [Self.srcTauri.appendingPathComponent("Info.ios.plist"), Self.appResources.appendingPathComponent("Info.plist")] {
            let info = try XCTUnwrap(NSDictionary(contentsOf: plist) as? [String: Any], plist.path)
            XCTAssertNil(info["NSContactsUsageDescription"], plist.path)
        }
    }
}
