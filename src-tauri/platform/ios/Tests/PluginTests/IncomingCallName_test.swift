import XCTest
@testable import tauri_plugin_ft_platform

// The name CallKit shows for a call PushKit reported before the core knows who calls
// (2026-10-01): "Incoming call" in the app's language. Seen in English on a Spanish iPhone, so
// every language the app ships must have the text, translated, and the lookup must use the app's
// tables. These read the app's `Localizable.strings` from the iOS project, one language at a time.
final class IncomingCallNameTests: XCTestCase {
    /// `src-tauri/gen/apple/flickertalk_iOS`, from this file's place in the repository.
    private static let appResources = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent() // PluginTests
        .deletingLastPathComponent() // Tests
        .deletingLastPathComponent() // ios
        .deletingLastPathComponent() // platform
        .deletingLastPathComponent() // src-tauri
        .appendingPathComponent("gen/apple/flickertalk_iOS")

    /// Every language the app ships, by its `.lproj` folder.
    private func languages() throws -> [String] {
        let folders = try FileManager.default.contentsOfDirectory(atPath: Self.appResources.path)
        return folders.filter { $0.hasSuffix(".lproj") }.map { String($0.dropLast(".lproj".count)) }.sorted()
    }

    /// The app's strings in one language, as a bundle made of that language's folder alone.
    private func bundle(_ language: String) throws -> Bundle {
        try XCTUnwrap(Bundle(url: Self.appResources.appendingPathComponent("\(language).lproj")), language)
    }

    private func table(_ language: String) throws -> [String: String] {
        let url = Self.appResources.appendingPathComponent("\(language).lproj/Localizable.strings")
        return try XCTUnwrap(NSDictionary(contentsOf: url) as? [String: String], "\(language): Localizable.strings does not parse")
    }

    // The app ships the languages of the app's catalogue (21: English and 20 more).
    func testTheAppShipsEveryLanguage() throws {
        let shipped = try languages()
        XCTAssertEqual(shipped.count, 21, "\(shipped)")
    }

    // A key missing in one language falls back to the development language, English, on a phone
    // in that language: every table has every key of the English one.
    func testEveryLanguageHasEveryText() throws {
        let english = try table("en")
        XCTAssertNotNil(english["FT_INCOMING_CALL"])
        XCTAssertNotNil(english["FT_INCOMING_VIDEO_CALL"])
        for language in try languages() {
            let missing = Set(english.keys).subtracting(try table(language).keys)
            XCTAssertTrue(missing.isEmpty, "\(language) lacks \(missing.sorted())")
        }
    }

    // The placeholder comes from the app's own tables, in the app's language: translated in every
    // language, never the key, and the caller's name when there is one.
    func testThePlaceholderIsTranslatedInEveryLanguage() throws {
        let englishText = try XCTUnwrap(try table("en")["FT_INCOMING_CALL"])
        for language in try languages() {
            let shown = incomingCallName(caller: "", bundle: try bundle(language))
            XCTAssertEqual(shown, try table(language)["FT_INCOMING_CALL"], language)
            XCTAssertNotEqual(shown, "FT_INCOMING_CALL", language)
            if language != "en" {
                XCTAssertNotEqual(shown, englishText, "\(language) shows the English text")
            }
        }
        XCTAssertEqual(incomingCallName(caller: "Ioan", bundle: try bundle("es")), "Ioan")
        XCTAssertEqual(try table("es")["FT_INCOMING_CALL"], "Llamada entrante")
    }
}
