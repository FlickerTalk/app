import Foundation
import NaturalLanguage
import SwiftUI
import Tauri
import Translation
import UIKit

// The text never leaves the phone. Language packs come from Apple's own sheet, after the user agreed.
// TranslationSession without a view is iOS 26+, so everything goes through one hidden view with .translationTask.

let detectConfidence = 0.5

let translateWait: TimeInterval = 30

let prepareWait: TimeInterval = 300

// closing Apple's sheet with ✕ returns like a finished download, so don't wait long for "installed"
let prepareInstalledWait: TimeInterval = 30

let installedPoll: TimeInterval = 0.5

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

@available(iOS 18, *)
func languageTag(_ language: Locale.Language) -> String {
    language.minimalIdentifier
}

@available(iOS 18, *)
func language(fromTag tag: String) -> Locale.Language {
    Locale.Language(identifier: tag)
}

@available(iOS 18, *)
func knownLanguage(_ tag: String, among listed: [String]) -> Locale.Language? {
    listed.contains(tag) ? language(fromTag: tag) : nil
}

@available(iOS 18, *)
func statusPayload(_ status: LanguageAvailability.Status) -> [String: Any] {
    switch status {
    case .installed: return ["status": "installed"]
    case .supported: return ["status": "downloadable"]
    case .unsupported: return ["status": "unsupported"]
    @unknown default: return ["status": "unsupported"]
    }
}

@available(iOS 18, *)
func detectPayload(_ hypotheses: [NLLanguage: Double]) -> [String: Any] {
    guard let best = hypotheses.max(by: { $0.value < $1.value }), best.key != .undetermined, best.value >= detectConfidence else {
        return ["found": false]
    }
    return ["found": true, "tag": languageTag(language(fromTag: best.key.rawValue))]
}

func textPayload(_ text: String?) -> [String: Any] {
    guard let text else { return ["found": false] }
    return ["found": true, "text": text]
}

@available(iOS 18, *)
func supportedTags() async -> [String] {
    var seen = Set<String>()
    return await LanguageAvailability().supportedLanguages.map(languageTag).filter { seen.insert($0).inserted }
}

@available(iOS 18, *)
private func knownPair(_ from: String, _ to: String) async -> (Locale.Language, Locale.Language)? {
    let listed = await supportedTags()
    guard let source = knownLanguage(from, among: listed), let target = knownLanguage(to, among: listed) else { return nil }
    return (source, target)
}

@available(iOS 18, *)
func pairStatus(from: String, to: String) async -> [String: Any] {
    guard let pair = await knownPair(from, to) else { return statusPayload(.unsupported) }
    let (source, target) = pair
    if from == to { return statusPayload(.installed) }
    return statusPayload(await LanguageAvailability().status(from: source, to: target))
}

@available(iOS 18, *)
func detectLanguage(_ text: String) -> [String: Any] {
    let recognizer = NLLanguageRecognizer()
    recognizer.processString(text)
    return detectPayload(recognizer.languageHypotheses(withMaximum: 1))
}

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
        let availability = LanguageAvailability()
        return ["ready": await becomesInstalled { await availability.status(from: source, to: target) == .installed }]
    }
}

func becomesInstalled(
    now: () -> Date = { Date() },
    sleep: (TimeInterval) async -> Void = { try? await Task.sleep(nanoseconds: UInt64($0 * 1_000_000_000)) },
    isInstalled: () async -> Bool
) async -> Bool {
    let deadline = now().addingTimeInterval(prepareInstalledWait)
    while true {
        if await isInstalled() { return true }
        if now() >= deadline || Task.isCancelled { return false }
        await sleep(installedPoll)
    }
}

// never brings up Apple's download sheet, prepare does that once the user agreed
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

@available(iOS 18, *)
private struct TranslationTaskView: View {
    let configuration: TranslationSession.Configuration
    let action: (TranslationSession) async -> Void

    var body: some View {
        Color.clear.translationTask(configuration, action: action)
    }
}

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

    // removing the view cancels the session's task if it's still running
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

extension PlatformPlugin {
    @objc public func translateLanguages(_ invoke: Invoke) throws {
        guard #available(iOS 18, *) else {
            invoke.resolve(["languages": [String]()])
            return
        }
        Task { invoke.resolve(["languages": await supportedTags()]) }
    }

    @objc public func translateDetect(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(TranslateSourceArgs.self)
        guard #available(iOS 18, *) else {
            invoke.resolve(["found": false])
            return
        }
        invoke.resolve(detectLanguage(args.text).mapValues { $0 as Any? })
    }

    @objc public func translateStatus(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(TranslatePairArgs.self)
        guard #available(iOS 18, *) else {
            invoke.resolve(["status": "unsupported"])
            return
        }
        Task { invoke.resolve(await pairStatus(from: args.from, to: args.to).mapValues { $0 as Any? }) }
    }

    @objc public func translatePrepare(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(TranslatePairArgs.self)
        guard #available(iOS 18, *) else {
            invoke.resolve(["ready": false])
            return
        }
        Task { @MainActor [manager] in
            invoke.resolve(await preparePair(from: args.from, to: args.to, on: manager.viewController).mapValues { $0 as Any? })
        }
    }

    // iOS keeps language packs, only the user can remove them in Settings
    @objc public func translateRemove(_ invoke: Invoke) throws {
        _ = try invoke.parseArgs(TranslatePairArgs.self)
        invoke.resolve(["removed": false])
    }

    @objc public func translateText(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(TranslateTextArgs.self)
        guard #available(iOS 18, *) else {
            invoke.resolve(["found": false])
            return
        }
        Task { @MainActor [manager] in
            invoke.resolve(await translateOnPhone(args.text, from: args.from, to: args.to, on: manager.viewController).mapValues { $0 as Any? })
        }
    }
}
