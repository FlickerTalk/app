import NaturalLanguage
import Translation
import XCTest
@testable import tauri_plugin_ft_platform

// On-device translation (2026-10-20, app#177): Apple's Translation framework, iOS 18 and up, for
// a plugin the user granted `translate`. Only the helpers are tested here: the simulator does
// not translate, so translating and downloading are tried on the iPhone.
final class TranslateTests: XCTestCase {
    // A language is named by its shortest tag, the same in the list and in a guess.
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

    // The bridge answers only for the tags it lists: a region, an unknown code or nothing is not
    // one of them, so it is unsupported rather than an error.
    func testOnlyTheListedTagsAreLanguages() throws {
        guard #available(iOS 18, *) else { throw XCTSkip("Translation needs iOS 18") }
        let listed = ["en", "es", "zh-TW"]
        XCTAssertEqual(knownLanguage("es", among: listed).map(languageTag), "es")
        XCTAssertEqual(knownLanguage("zh-TW", among: listed).map(languageTag), "zh-TW")
        XCTAssertNil(knownLanguage("es-419", among: listed))
        XCTAssertNil(knownLanguage("xx", among: listed))
        XCTAssertNil(knownLanguage("", among: listed))
    }

    // Per pair, as Apple gives it: on the phone, after a download (Apple's sheet), or not here.
    // What `translateStatus` resolves with (`PairState` in platform/src/lib.rs).
    func testAPairIsInstalledDownloadableOrUnsupported() throws {
        guard #available(iOS 18, *) else { throw XCTSkip("Translation needs iOS 18") }
        XCTAssertEqual(statusPayload(.installed) as NSDictionary, ["status": "installed"] as NSDictionary)
        XCTAssertEqual(statusPayload(.supported) as NSDictionary, ["status": "downloadable"] as NSDictionary)
        XCTAssertEqual(statusPayload(.unsupported) as NSDictionary, ["status": "unsupported"] as NSDictionary)
    }

    // What `translateDetect` resolves with (`Detected` in platform/src/lib.rs): NaturalLanguage's
    // guess, named as the list names it, at even odds or better.
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

    // What `translateText` resolves with (`Translated` in platform/src/lib.rs).
    func testATranslationGoesBackAsFoundAndNothingAsNotFound() {
        XCTAssertEqual(textPayload("hello") as NSDictionary, ["found": true, "text": "hello"] as NSDictionary)
        XCTAssertEqual(textPayload(nil) as NSDictionary, ["found": false] as NSDictionary)
    }

    // One request at a time goes through the hidden SwiftUI view; one that never answers gives
    // up after the core's own limits, so the next is never stuck behind it. Apple's sheet may stay
    // up for the whole `prepare` limit; once it is gone, the pair gets a short wait of its own.
    func testARequestGivesUpAfterTheCoresLimits() {
        XCTAssertEqual(translateWait, 30)
        XCTAssertEqual(prepareWait, 300)
        XCTAssertEqual(prepareInstalledWait, 30)
    }

    // The user closed Apple's sheet with ✕ (`prepareTranslation()` returns, nothing downloads):
    // the pair is not ready after 30 s, not after the sheet's 5 minutes, so the plugin's hook and
    // the conversation's queue behind it move on. The user can tap again later.
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

    // A download that finishes behind the sheet is ready as soon as the pair shows as installed.
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

    // A pair already installed when the sheet goes needs no wait at all.
    func testAPairAlreadyInstalledNeedsNoWait() async {
        let clock = TestClock()
        let installed = await becomesInstalled(now: { clock.now }, sleep: { clock.now += $0 }) { true }
        XCTAssertTrue(installed)
        XCTAssertEqual(clock.elapsed, 0)
    }

    // The commands Rust runs (`run_mobile_plugin` in platform/src/lib.rs), as Tauri looks them
    // up: a name that differs would only show on the iPhone.
    func testTheTranslateCommandsHaveTheNamesRustSends() {
        for name in ["translateLanguages", "translateDetect", "translateStatus", "translatePrepare", "translateRemove", "translateText"] {
            let found = PlatformPlugin.instancesRespond(to: NSSelectorFromString("\(name):error:"))
                || PlatformPlugin.instancesRespond(to: NSSelectorFromString("\(name):"))
            XCTAssertTrue(found, name)
        }
    }
}

/// A clock that only moves when the code under test sleeps.
private final class TestClock {
    let start = Date(timeIntervalSince1970: 0)
    var now = Date(timeIntervalSince1970: 0)
    var elapsed: TimeInterval { now.timeIntervalSince(start) }
}
