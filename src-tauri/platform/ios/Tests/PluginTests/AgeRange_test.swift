import XCTest
@testable import tauri_plugin_ft_platform

// 2026-10-07: minor or adult is what the system says (Declared Age Range, iOS 26+), never what
// the user declares. The app asks with the single gate 18: a minor gets nil...17, an adult 18...nil.
final class AgeRangeTests: XCTestCase {
    /// The app's entitlements, from this file's place in the repository.
    private static let entitlements = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent() // PluginTests
        .deletingLastPathComponent() // Tests
        .deletingLastPathComponent() // ios
        .deletingLastPathComponent() // platform
        .deletingLastPathComponent() // src-tauri
        .appendingPathComponent("gen/apple/flickertalk_iOS/flickertalk_iOS.entitlements")

    func testTheAppAsksWithTheSingleAdultGate() {
        XCTAssertEqual(adultAge, 18)
    }

    func testARangeBelowTheGateIsAMinorAndOneFromItAnAdult() {
        XCTAssertEqual(ageClassOf(sharing: true, lower: nil, upper: 17), "minor")
        XCTAssertEqual(ageClassOf(sharing: true, lower: 18, upper: nil), "adult")
    }

    // The system may use its region's own gates (13, 16, 18): the same rule reads them.
    func testARegionsOwnBandsReadTheSameWay() {
        XCTAssertEqual(ageClassOf(sharing: true, lower: nil, upper: 12), "minor")
        XCTAssertEqual(ageClassOf(sharing: true, lower: 13, upper: 15), "minor")
        XCTAssertEqual(ageClassOf(sharing: true, lower: 16, upper: 17), "minor")
        XCTAssertEqual(ageClassOf(sharing: true, lower: 21, upper: nil), "adult")
    }

    // Declined, nothing shared or a range that straddles 18: no guess.
    func testAnythingElseIsUnknown() {
        XCTAssertEqual(ageClassOf(sharing: false, lower: nil, upper: 17), "unknown")
        XCTAssertEqual(ageClassOf(sharing: false, lower: 18, upper: nil), "unknown")
        XCTAssertEqual(ageClassOf(sharing: true, lower: nil, upper: nil), "unknown")
        XCTAssertEqual(ageClassOf(sharing: true, lower: 16, upper: nil), "unknown")
    }

    // Without the entitlement the system refuses the request (the Declared Age Range capability).
    func testTheAppCarriesTheDeclaredAgeRangeEntitlement() throws {
        let plist = try XCTUnwrap(NSDictionary(contentsOf: Self.entitlements) as? [String: Any])
        XCTAssertEqual(plist["com.apple.developer.declared-age-range"] as? Bool, true)
    }
}
