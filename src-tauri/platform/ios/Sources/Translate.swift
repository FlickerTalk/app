import Foundation
import NaturalLanguage
import SwiftUI
import Translation
import UIKit

// On-device translation (2026-10-20, Plan §53, app#177): Apple's Translation framework, iOS 18
// and up, for a plugin the user granted `translate` (the core checked before asking here). The
// text never leaves the phone. A language pack comes from Apple once, with Apple's own sheet,
// after the user agreed in the plugin; iOS keeps the packs and the user removes them in Settings.
// Before iOS 18 nothing translates: no languages. The bridge answers only for the tags it lists;
// any other tag is unsupported, never an error.
//
// A `TranslationSession` without a view (`init(installedSource:target:)`) is iOS 26 only, so every
// download and translation goes through one hidden SwiftUI view with `.translationTask`, one
// request at a time (`TranslationHost`).

/// A guess below even odds is no guess.
let detectConfidence = 0.5

/// How long a translation may take before the request gives up (the core's own limit).
let translateWait: TimeInterval = 30

/// How long a download may take, the user's time on Apple's sheet included.
let prepareWait: TimeInterval = 300

class TranslatePairArgs: Decodable {
    let from: String
    let to: String
}

class TranslateSourceArgs: Decodable {
    let text: String
}

class TranslateTextArgs: Decodable {
    let text: String
    let from: String
    let to: String
}

/// A language's tag: its shortest form ("es", "zh", "zh-TW", "en-GB").
@available(iOS 18, *)
func languageTag(_ language: Locale.Language) -> String {
    language.minimalIdentifier
}

@available(iOS 18, *)
func language(fromTag tag: String) -> Locale.Language {
    Locale.Language(identifier: tag)
}

/// The language of one of the tags this phone lists; any other ("es-419", "xx", "") is none.
@available(iOS 18, *)
func knownLanguage(_ tag: String, among listed: [String]) -> Locale.Language? {
    listed.contains(tag) ? language(fromTag: tag) : nil
}

/// What `translateStatus` resolves with (`PairState` in platform/src/lib.rs).
@available(iOS 18, *)
func statusPayload(_ status: LanguageAvailability.Status) -> [String: Any] {
    switch status {
    case .installed: return ["status": "installed"]
    case .supported: return ["status": "downloadable"]
    case .unsupported: return ["status": "unsupported"]
    @unknown default: return ["status": "unsupported"]
    }
}

/// What `translateDetect` resolves with (`Detected` in platform/src/lib.rs): the likeliest
/// language, named as the list names it, at even odds or better.
@available(iOS 18, *)
func detectPayload(_ hypotheses: [NLLanguage: Double]) -> [String: Any] {
    guard let best = hypotheses.max(by: { $0.value < $1.value }), best.key != .undetermined, best.value >= detectConfidence else {
        return ["found": false]
    }
    return ["found": true, "tag": languageTag(language(fromTag: best.key.rawValue))]
}

/// What `translateText` resolves with (`Translated` in platform/src/lib.rs).
func textPayload(_ text: String?) -> [String: Any] {
    guard let text else { return ["found": false] }
    return ["found": true, "text": text]
}

/// The tags this phone translates between, downloaded or not.
@available(iOS 18, *)
func supportedTags() async -> [String] {
    var seen = Set<String>()
    return await LanguageAvailability().supportedLanguages.map(languageTag).filter { seen.insert($0).inserted }
}

/// Both languages of a pair, if this phone lists them.
@available(iOS 18, *)
private func knownPair(_ from: String, _ to: String) async -> (Locale.Language, Locale.Language)? {
    let listed = await supportedTags()
    guard let source = knownLanguage(from, among: listed), let target = knownLanguage(to, among: listed) else { return nil }
    return (source, target)
}

/// `translateStatus`: the pair's state; a text already in the language asked for needs nothing.
@available(iOS 18, *)
func pairStatus(from: String, to: String) async -> [String: Any] {
    guard let pair = await knownPair(from, to) else { return statusPayload(.unsupported) }
    let (source, target) = pair
    if from == to { return statusPayload(.installed) }
    return statusPayload(await LanguageAvailability().status(from: source, to: target))
}

/// `translateDetect`: NaturalLanguage's guess, on the phone.
@available(iOS 18, *)
func detectLanguage(_ text: String) -> [String: Any] {
    let recognizer = NLLanguageRecognizer()
    recognizer.processString(text)
    return detectPayload(recognizer.languageHypotheses(withMaximum: 1))
}

/// `translatePrepare`: asks Apple for the pair's packs (its sheet asks the user) and says whether
/// the pair can be used now.
@available(iOS 18, *)
@MainActor
func preparePair(from: String, to: String, on screen: UIViewController?) async -> [String: Any] {
    guard let pair = await knownPair(from, to) else { return ["ready": false] }
    let (source, target) = pair
    if from == to { return ["ready": true] }
    switch await LanguageAvailability().status(from: source, to: target) {
    case .installed: return ["ready": true]
    case .supported: break
    default: return ["ready": false]
    }
    return await TranslationHost.shared.run(source, target, limit: prepareWait, fallback: ["ready": false], on: screen) { session in
        do {
            try await session.prepareTranslation()
        } catch {
            return ["ready": false] // declined, or the download failed
        }
        return ["ready": await becomesInstalled(source, target, within: prepareWait)]
    }
}

/// Apple's sheet may return while the download goes on: the pair is ready once it is installed.
@available(iOS 18, *)
private func becomesInstalled(_ source: Locale.Language, _ target: Locale.Language, within seconds: TimeInterval) async -> Bool {
    let availability = LanguageAvailability()
    let deadline = Date().addingTimeInterval(seconds)
    while true {
        if await availability.status(from: source, to: target) == .installed { return true }
        if Date() >= deadline || Task.isCancelled { return false }
        try? await Task.sleep(nanoseconds: 500_000_000)
    }
}

/// `translateText`: translates on the phone, only with a pair already on it (a translation never
/// brings up Apple's download sheet: `prepare` does that, after the user agreed).
@available(iOS 18, *)
@MainActor
func translateOnPhone(_ text: String, from: String, to: String, on screen: UIViewController?) async -> [String: Any] {
    guard let pair = await knownPair(from, to) else { return textPayload(nil) }
    let (source, target) = pair
    if from == to { return textPayload(text) }
    guard await LanguageAvailability().status(from: source, to: target) == .installed else { return textPayload(nil) }
    return await TranslationHost.shared.run(source, target, limit: translateWait, fallback: textPayload(nil), on: screen) { session in
        textPayload(try? await session.translate(text).targetText)
    }
}

/// The hidden view a `TranslationSession` comes from.
@available(iOS 18, *)
private struct TranslationTaskView: View {
    let configuration: TranslationSession.Configuration
    let action: (TranslationSession) async -> Void

    var body: some View {
        Color.clear.translationTask(configuration, action: action)
    }
}

/// Runs one request at a time through a hidden SwiftUI view (alpha 0, no touches) added under the
/// app's screen; it leaves once the request is done or its time is up, and the next one starts.
@available(iOS 18, *)
@MainActor
final class TranslationHost {
    static let shared = TranslationHost()

    private struct Request {
        let configuration: TranslationSession.Configuration
        let limit: TimeInterval
        let fallback: [String: Any]
        let screen: UIViewController?
        let work: (TranslationSession) async -> [String: Any]
        let done: ([String: Any]) -> Void
    }

    private var waiting: [Request] = []
    private var running: (id: UUID, view: UIViewController, request: Request)?

    /// What `work` answers with the pair's session, or `fallback` if there is no screen to host
    /// the view or the time is up.
    func run(
        _ source: Locale.Language,
        _ target: Locale.Language,
        limit: TimeInterval,
        fallback: [String: Any],
        on screen: UIViewController?,
        work: @escaping (TranslationSession) async -> [String: Any]
    ) async -> [String: Any] {
        await withCheckedContinuation { continuation in
            waiting.append(Request(
                configuration: TranslationSession.Configuration(source: source, target: target),
                limit: limit,
                fallback: fallback,
                screen: screen,
                work: work,
                done: { continuation.resume(returning: $0) }
            ))
            next()
        }
    }

    private func next() {
        guard running == nil, !waiting.isEmpty else { return }
        let request = waiting.removeFirst()
        guard let screen = request.screen else {
            request.done(request.fallback)
            return next()
        }
        let id = UUID()
        let host = UIHostingController(rootView: TranslationTaskView(configuration: request.configuration) { @MainActor session in
            let answer = await request.work(session)
            self.finish(id, with: answer)
        })
        host.view.frame = CGRect(x: 0, y: 0, width: 1, height: 1)
        host.view.alpha = 0
        host.view.isUserInteractionEnabled = false
        screen.addChild(host)
        screen.view.addSubview(host.view)
        host.didMove(toParent: screen)
        running = (id, host, request)
        Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(request.limit * 1_000_000_000))
            self?.finish(id, with: request.fallback)
        }
    }

    /// Answers once: the first of the work and the clock wins; the view leaves, which cancels the
    /// session's task if it still runs.
    private func finish(_ id: UUID, with answer: [String: Any]) {
        guard let current = running, current.id == id else { return }
        running = nil
        current.view.willMove(toParent: nil)
        current.view.view.removeFromSuperview()
        current.view.removeFromParent()
        current.request.done(answer)
        next()
    }
}
