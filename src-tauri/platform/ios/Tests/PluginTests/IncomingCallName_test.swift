import XCTest
@testable import tauri_plugin_ft_platform

final class IncomingCallNameTests: XCTestCase {
    private static let appResources = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .appendingPathComponent("gen/apple/flickertalk_iOS")

    private func languages() throws -> [String] {
        let folders = try FileManager.default.contentsOfDirectory(atPath: Self.appResources.path)
        return folders.filter { $0.hasSuffix(".lproj") }.map { String($0.dropLast(".lproj".count)) }.sorted()
    }

    private func bundle(_ language: String) throws -> Bundle {
        try XCTUnwrap(Bundle(url: Self.appResources.appendingPathComponent("\(language).lproj")), language)
    }

    private func table(_ language: String) throws -> [String: String] {
        let url = Self.appResources.appendingPathComponent("\(language).lproj/Localizable.strings")
        return try XCTUnwrap(NSDictionary(contentsOf: url) as? [String: String], "\(language): Localizable.strings does not parse")
    }

    func testTheAppShipsEveryLanguage() throws {
        let shipped = try languages()
        XCTAssertEqual(shipped.count, 21, "\(shipped)")
    }

    func testEveryLanguageHasEveryText() throws {
        let english = try table("en")
        XCTAssertNotNil(english["FT_INCOMING_CALL"])
        XCTAssertNotNil(english["FT_INCOMING_VIDEO_CALL"])
        for language in try languages() {
            let missing = Set(english.keys).subtracting(try table(language).keys)
            XCTAssertTrue(missing.isEmpty, "\(language) lacks \(missing.sorted())")
        }
    }

    func testEveryLanguageTranslatesEveryText() throws {
        let english = try table("en")
        var untranslated: [String: [String]] = [:]
        for language in try languages() where language != "en" {
            let same = try table(language).filter { english[$0.key] == $0.value }.keys.sorted()
            if !same.isEmpty { untranslated[language] = same }
        }
        XCTAssertEqual(untranslated, [:], "texts still in English")
    }

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
        XCTAssertEqual(incomingCallName(caller: "Alice", bundle: try bundle("es")), "Alice")
        XCTAssertEqual(try table("es")["FT_INCOMING_CALL"], "Llamada entrante")
    }
}
