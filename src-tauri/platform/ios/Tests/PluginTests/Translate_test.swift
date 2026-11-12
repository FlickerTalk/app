import NaturalLanguage
import Translation
import XCTest
@testable import tauri_plugin_ft_platform

final class TranslateTests: XCTestCase {
    func testALanguageIsNamedByItsShortestTag() throws {
        guard #available(iOS 18, *) else { throw XCTSkip("Translation needs iOS 18") }
        XCTAssertEqual(languageTag(Locale.Language(identifier: "es-ES")), "es")
        XCTAssertEqual(languageTag(Locale.Language(identifier: "en-US")), "en")
        XCTAssertEqual(languageTag(Locale.Language(identifier: "en-GB")), "en-GB")
        XCTAssertEqual(languageTag(Locale.Language(identifier: "pt-BR")), "pt")
        XCTAssertEqual(languageTag(Locale.Language(identifier: "zh-Hans")), "zh")
        XCTAssertEqual(languageTag(Locale.Language(identifier: "zh-Hant")), "zh-TW")
        XCTAssertEqual(languageTag(language(fromTag: "zh-TW")), "zh-TW")
    }

    func testOnlyTheListedTagsAreLanguages() throws {
        guard #available(iOS 18, *) else { throw XCTSkip("Translation needs iOS 18") }
        let listed = ["en", "es", "zh-TW"]
        XCTAssertEqual(knownLanguage("es", among: listed).map(languageTag), "es")
        XCTAssertEqual(knownLanguage("zh-TW", among: listed).map(languageTag), "zh-TW")
        XCTAssertNil(knownLanguage("es-419", among: listed))
        XCTAssertNil(knownLanguage("xx", among: listed))
        XCTAssertNil(knownLanguage("", among: listed))
    }

    func testAPairIsInstalledDownloadableOrUnsupported() throws {
        guard #available(iOS 18, *) else { throw XCTSkip("Translation needs iOS 18") }
        XCTAssertEqual(statusPayload(.installed) as NSDictionary, ["status": "installed"] as NSDictionary)
        XCTAssertEqual(statusPayload(.supported) as NSDictionary, ["status": "downloadable"] as NSDictionary)
        XCTAssertEqual(statusPayload(.unsupported) as NSDictionary, ["status": "unsupported"] as NSDictionary)
    }

    func testAGuessGoesBackOnlyWhenItIsLikelyEnough() throws {
        guard #available(iOS 18, *) else { throw XCTSkip("Translation needs iOS 18") }
        XCTAssertEqual(detectPayload([.spanish: 0.9]) as NSDictionary, ["found": true, "tag": "es"] as NSDictionary)
        XCTAssertEqual(detectPayload([.simplifiedChinese: 0.8, .japanese: 0.1]) as NSDictionary, ["found": true, "tag": "zh"] as NSDictionary)
        XCTAssertEqual(detectPayload([.traditionalChinese: 0.7]) as NSDictionary, ["found": true, "tag": "zh-TW"] as NSDictionary)
        XCTAssertEqual(detectPayload([.spanish: 0.5]) as NSDictionary, ["found": true, "tag": "es"] as NSDictionary)
        XCTAssertEqual(detectPayload([.spanish: 0.4, .portuguese: 0.35]) as NSDictionary, ["found": false] as NSDictionary)
        XCTAssertEqual(detectPayload([.undetermined: 1]) as NSDictionary, ["found": false] as NSDictionary)
        XCTAssertEqual(detectPayload([:]) as NSDictionary, ["found": false] as NSDictionary)
        XCTAssertEqual(detectConfidence, 0.5)
    }

    func testATranslationGoesBackAsFoundAndNothingAsNotFound() {
        XCTAssertEqual(textPayload("hello") as NSDictionary, ["found": true, "text": "hello"] as NSDictionary)
        XCTAssertEqual(textPayload(nil) as NSDictionary, ["found": false] as NSDictionary)
    }

    func testARequestGivesUpAfterTheCoresLimits() {
        XCTAssertEqual(translateWait, 30)
        XCTAssertEqual(prepareWait, 300)
        XCTAssertEqual(prepareInstalledWait, 30)
    }

    func testOnceTheSheetIsGoneAPairThatNeverInstallsIsNotReadyAfterThirtySeconds() async {
        let clock = TestClock()
        var asks = 0
        let installed = await becomesInstalled(now: { clock.now }, sleep: { clock.now += $0 }) {
            asks += 1
            return false
        }
        XCTAssertFalse(installed)
        XCTAssertEqual(clock.elapsed, 30, accuracy: installedPoll)
        XCTAssertGreaterThan(asks, 1)
    }

    func testAPairThatInstallsWhileWaitingIsReadyAtOnce() async {
        let clock = TestClock()
        var asks = 0
        let installed = await becomesInstalled(now: { clock.now }, sleep: { clock.now += $0 }) {
            asks += 1
            return asks == 3
        }
        XCTAssertTrue(installed)
        XCTAssertEqual(asks, 3)
        XCTAssertEqual(clock.elapsed, 2 * installedPoll, accuracy: 0.001)
    }

    func testAPairAlreadyInstalledNeedsNoWait() async {
        let clock = TestClock()
        let installed = await becomesInstalled(now: { clock.now }, sleep: { clock.now += $0 }) { true }
        XCTAssertTrue(installed)
        XCTAssertEqual(clock.elapsed, 0)
    }

    func testTheTranslateCommandsHaveTheNamesRustSends() {
        for name in ["translateLanguages", "translateDetect", "translateStatus", "translatePrepare", "translateRemove", "translateText"] {
            let found = PlatformPlugin.instancesRespond(to: NSSelectorFromString("\(name):error:"))
                || PlatformPlugin.instancesRespond(to: NSSelectorFromString("\(name):"))
            XCTAssertTrue(found, name)
        }
    }
}

private final class TestClock {
    let start = Date(timeIntervalSince1970: 0)
    var now = Date(timeIntervalSince1970: 0)
    var elapsed: TimeInterval { now.timeIntervalSince(start) }
}
