import CoreLocation
import XCTest
@testable import tauri_plugin_ft_platform

// The location plugin (2026-10-02): the phone's current position, once, while the app is open.
// "While using the app" only: no "always", no background mode, no following.
final class LocationTests: XCTestCase {
    /// `src-tauri`, from this file's place in the repository.
    private static let srcTauri = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent() // PluginTests
        .deletingLastPathComponent() // Tests
        .deletingLastPathComponent() // ios
        .deletingLastPathComponent() // platform
        .deletingLastPathComponent() // src-tauri
    private static let appResources = srcTauri.appendingPathComponent("gen/apple/flickertalk_iOS")

    // Asked only if it was never asked; a no (or a restriction) stays a no.
    func testLocationIsAskedOnlyWhenItWasNeverAsked() {
        XCTAssertEqual(locationAccess(.notDetermined), .ask)
        XCTAssertEqual(locationAccess(.authorizedWhenInUse), .granted)
        XCTAssertEqual(locationAccess(.authorizedAlways), .granted)
        XCTAssertEqual(locationAccess(.denied), .denied)
        XCTAssertEqual(locationAccess(.restricted), .denied)
    }

    // What `currentLocation` resolves with, as Rust reads it (`Located` in platform/src/lib.rs).
    func testAFixGoesBackAsFoundWithItsPlaceAndNothingAsNotFound() {
        XCTAssertEqual(locationPayload(nil) as NSDictionary, ["found": false] as NSDictionary)
        let madrid = CLLocation(
            coordinate: CLLocationCoordinate2D(latitude: 40.41678, longitude: -3.70379),
            altitude: 0, horizontalAccuracy: 35, verticalAccuracy: -1,
            timestamp: Date(timeIntervalSince1970: 1_790_000_000)
        )
        XCTAssertEqual(
            locationPayload(madrid) as NSDictionary,
            ["found": true, "lat": 40.41678, "lon": -3.70379, "accuracy": 35.0, "at": 1_790_000_000_000.0] as NSDictionary
        )
        // A negative accuracy is CoreLocation's way of saying the fix is not valid.
        let invalid = CLLocation(
            coordinate: CLLocationCoordinate2D(latitude: 40.4, longitude: -3.7),
            altitude: 0, horizontalAccuracy: -1, verticalAccuracy: -1, timestamp: Date()
        )
        XCTAssertEqual(locationPayload(invalid) as NSDictionary, ["found": false] as NSDictionary)
    }

    func testTheUserWaitsForAFixAboutFifteenSeconds() {
        XCTAssertEqual(locationTimeout, 15)
    }

    // Only "while using the app", in both the plist Tauri merges and the one Xcode builds.
    func testThePlistsAskForLocationWhileInUseOnly() throws {
        for plist in [Self.srcTauri.appendingPathComponent("Info.ios.plist"), Self.appResources.appendingPathComponent("Info.plist")] {
            let info = try XCTUnwrap(NSDictionary(contentsOf: plist) as? [String: Any], plist.path)
            XCTAssertNotNil(info["NSLocationWhenInUseUsageDescription"], plist.path)
            XCTAssertNil(info["NSLocationAlwaysAndWhenInUseUsageDescription"], plist.path)
            XCTAssertNil(info["NSLocationAlwaysUsageDescription"], plist.path)
            let modes = info["UIBackgroundModes"] as? [String] ?? []
            XCTAssertFalse(modes.contains("location"), plist.path)
        }
    }

    // The question iOS shows speaks the app's languages, like the camera's and the microphone's.
    func testTheLocationQuestionIsTranslatedInEveryLanguage() throws {
        let folders = try FileManager.default.contentsOfDirectory(atPath: Self.appResources.path).filter { $0.hasSuffix(".lproj") }
        XCTAssertEqual(folders.count, 21)
        func text(_ folder: String) throws -> String? {
            let url = Self.appResources.appendingPathComponent("\(folder)/InfoPlist.strings")
            let table = try XCTUnwrap(NSDictionary(contentsOf: url) as? [String: String], "\(folder): InfoPlist.strings does not parse")
            return table["NSLocationWhenInUseUsageDescription"]
        }
        let english = try XCTUnwrap(try text("en.lproj"))
        for folder in folders where folder != "en.lproj" {
            let translated = try XCTUnwrap(try text(folder), folder)
            XCTAssertFalse(translated.isEmpty, folder)
            XCTAssertNotEqual(translated, english, "\(folder) shows the English text")
        }
    }
}
