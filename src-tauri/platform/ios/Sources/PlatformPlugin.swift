import AVFAudio
import AVFoundation
import AuthenticationServices
import CallKit
import Foundation
import PushKit
import ObjectiveC
import QuickLook
import Security
import StoreKit
import SwiftRs
import Tauri
import UIKit
import UserNotifications
import WebKit

private let keyService = "com.flickertalk.app.storage"
private let keyAccount = "storage-key"

// Rust only stores this marker, the key itself stays in the Keychain
func keychainMarker() -> Data {
    Data("keychain:v1".utf8)
}

// ThisDeviceOnly, so it's never synced to iCloud or restored on another device
func keyAccessibility() -> String {
    kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly as String
}

func shareableText(_ text: String) -> String? {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}

// product id in App Store Connect
let yearly = "yearly"

struct StoreEntitlement {
    let product: String
    let expires: Date?
    let revoked: Date?
}

// ms
func activeUntil(_ entitlements: [StoreEntitlement], now: Date) -> Int64 {
    entitlements
        .filter { $0.product == yearly && $0.revoked == nil }
        .compactMap { $0.expires }
        .filter { $0 > now }
        .map { Int64(($0.timeIntervalSince1970 * 1000).rounded()) }
        .max() ?? 0
}

struct StoreProduct {
    let id: String
    let displayPrice: String
}

func yearlyPrice(_ products: [StoreProduct]) -> String? {
    guard let price = products.first(where: { $0.id == yearly })?.displayPrice
        .trimmingCharacters(in: .whitespacesAndNewlines), !price.isEmpty else { return nil }
    return price
}

func restoreTrouble(_ error: Error) -> String {
    if case StoreKitError.userCancelled = error { return "cancelled" }
    return "store_unavailable"
}

final class EntitlementQueue {
    private var sink: ((Int64) -> Void)?
    private var latest: Int64?

    func register(_ sink: @escaping (Int64) -> Void) {
        self.sink = sink
        if let latest {
            self.latest = nil
            sink(latest)
        }
    }

    func offer(_ until: Int64) {
        if let sink {
            sink(until)
        } else {
            latest = until
        }
    }
}

func currentEntitlements() async -> [StoreEntitlement] {
    var found: [StoreEntitlement] = []
    for await result in Transaction.currentEntitlements {
        guard case .verified(let transaction) = result else { continue }
        found.append(
            StoreEntitlement(
                product: transaction.productID,
                expires: transaction.expirationDate,
                revoked: transaction.revocationDate
            )
        )
    }
    return found
}

// Apple wants transaction updates listened to from launch, or transactions stay unfinished
final class StoreUpdates {
    static let shared = StoreUpdates()
    private let queue = DispatchQueue(label: "com.flickertalk.store.updates")
    private let entitlements = EntitlementQueue()
    private var started = false
    private var observer: NSObjectProtocol?

    func register(_ channel: Channel) {
        queue.async { self.entitlements.register { channel.send(["until": $0] as JsonObject) } }
    }

    func start() {
        queue.sync {
            guard !started else { return }
            started = true
        }
        Task.detached {
            for await result in Transaction.updates {
                // only finish what Apple signed, then reread the entitlements either way
                if case .verified(let transaction) = result { await transaction.finish() }
                await StoreUpdates.shared.readNow()
            }
        }
        // an expiry is no transaction, it only shows in the subscription status (iOS 17)
        if #available(iOS 17.0, *) {
            Task.detached {
                for await _ in Product.SubscriptionInfo.Status.updates {
                    await StoreUpdates.shared.readNow()
                }
            }
        }
        DispatchQueue.main.async {
            self.observer = NotificationCenter.default.addObserver(
                forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main
            ) { _ in
                Task { await StoreUpdates.shared.readNow() }
            }
        }
    }

    func readNow() async {
        let until = activeUntil(await currentEntitlements(), now: Date())
        queue.async { self.entitlements.offer(until) }
    }
}


struct ReminderEntry: Decodable, Equatable {
    let plugin: String
    let id: String
    let at: Int64
    let text: String?
}

func parseReminders(_ json: String) -> [ReminderEntry] {
    guard let data = json.data(using: .utf8),
          let entries = try? JSONDecoder().decode([ReminderEntry].self, from: data) else { return [] }
    return entries.filter { !$0.plugin.isEmpty && !$0.id.isEmpty && $0.at > 0 }
}

func reminderIdentifier(_ entry: ReminderEntry) -> String { "ft.reminder|" + entry.plugin + "|" + entry.id }

func reminderKey(_ identifier: String) -> String? {
    let parts = identifier.split(separator: "|", maxSplits: 2).map(String.init)
    guard parts.count == 3, parts[0] == "ft.reminder" else { return nil }
    return parts[1] + "\n" + parts[2]
}

// iOS keeps at most 64 pending notifications, so set the soonest
func remindersToSchedule(_ entries: [ReminderEntry], now: Int64, limit: Int = 64) -> [ReminderEntry] {
    Array(entries.filter { $0.at > now }.sorted { $0.at < $1.at }.prefix(limit))
}

func isAuthRedirect(_ url: URL?, scheme: String) -> Bool {
    guard let url, !scheme.isEmpty else { return false }
    return url.scheme?.lowercased() == scheme.lowercased()
}

class RemindersArgs: Decodable {
    let reminders: String
}

class AuthorizeArgs: Decodable {
    let url: String
    let scheme: String
}

class ShareArgs: Decodable {
    let text: String
}

class KeyArgs: Decodable {
    let value: String
}

enum KeychainError: Error {
    case status(OSStatus)
    case notFound
}

func keyQuery() -> [String: Any] {
    [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: keyService,
        kSecAttrAccount as String: keyAccount,
    ]
}

func storeKey(_ key: Data) throws {
    SecItemDelete(keyQuery() as CFDictionary)
    var item = keyQuery()
    item[kSecValueData as String] = key
    item[kSecAttrAccessible as String] = keyAccessibility()
    let status = SecItemAdd(item as CFDictionary, nil)
    guard status == errSecSuccess else { throw KeychainError.status(status) }
}

func loadKey() throws -> Data {
    var query = keyQuery()
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne
    var result: AnyObject?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    guard status == errSecSuccess else { throw status == errSecItemNotFound ? KeychainError.notFound : KeychainError.status(status) }
    guard let key = result as? Data else { throw KeychainError.notFound }
    return key
}

final class ReminderTaps: NSObject, UNUserNotificationCenterDelegate {
    static let shared = ReminderTaps()
    var pending: String = ""

    // iOS may call this off the main thread and UIKit wants the answer on main, otherwise a tap killed the app
    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
        tapped(identifier: response.notification.request.identifier, done: completionHandler)
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        presenting(userInfo: notification.request.content.userInfo, done: completionHandler)
    }

    func tapped(identifier: String, done: @escaping () -> Void) {
        DispatchQueue.main.async {
            if let key = reminderKey(identifier) { self.pending = key }
            done()
        }
    }

    func presenting(userInfo: [AnyHashable: Any], done: @escaping (UNNotificationPresentationOptions) -> Void) {
        let options: UNNotificationPresentationOptions = isWakePush(userInfo) ? [] : [.banner, .sound, .list]
        DispatchQueue.main.async { done(options) }
    }
}

// development builds carry aps-environment development in the profile, App Store builds have none
func apnsGateway(provisioning: String?) -> String {
    guard let profile = provisioning else { return "production" }
    let development = "<key>aps-environment</key>\\s*<string>development</string>"
    return profile.range(of: development, options: .regularExpression) != nil ? "sandbox" : "production"
}

private func hex(_ data: Data) -> String {
    data.map { String(format: "%02x", $0) }.joined()
}

func pushTarget(gateway: String, bundle: String, token: Data, voip: Data? = nil) -> String {
    let target = "\(gateway):\(bundle):\(hex(token))"
    return voip.map { "\(target):\(hex($0))" } ?? target
}

func isCallPush(_ userInfo: [AnyHashable: Any]) -> Bool {
    (userInfo["t"] as? String) == "call"
}

// CallKit rings even with the app on screen, so answering takes one path.
// Quiet calls never go through CallKit, which can't ring silently.
enum Ring: Equatable {
    case report
    case update
    case silent
    case endAndSilent
}

func ringWith(callKitCall: Bool, answered: Bool = false, quiet: Bool = false) -> Ring {
    if quiet && !answered { return callKitCall ? .endAndSilent : .silent }
    return callKitCall ? .update : .report
}

// through CallKit the audio session comes up as it does on the lock screen
func answerThroughCallKit(callKitCall: Bool, answered: Bool, outgoing: Bool) -> Bool {
    callKitCall && !answered && !outgoing
}

// .none is the receiver under .voiceChat
func outputOverride(speaker: Bool) -> AVAudioSession.PortOverride {
    speaker ? .speaker : .none
}

func restartAfterInterruption(shouldResume: Bool, reactivated: Bool, callLive: Bool) -> Bool {
    shouldResume && !reactivated && callLive
}

// call media lives in Rust, no WebView in between, so a call answered on a locked iPhone has audio

// mirrors NativeCallEvent in Rust
enum CallEvent: Equatable {
    case incoming
    case answer
    case end
    case decline
    case mute(Bool)
    // call generation, so Rust can drop late events from an older call
    case audioActivated(UInt64)
    case audioDeactivated(UInt64)
    case visible(Bool)
    // UIDeviceOrientation.rawValue
    case orientation(Int)
    case videoRequested
    case route(CallOutput)
}

func callEventPayload(_ event: CallEvent) -> JsonObject {
    switch event {
    case .incoming: return ["event": "incoming"]
    case .answer: return ["event": "answer"]
    case .end: return ["event": "end"]
    case .decline: return ["event": "decline"]
    case .mute(let muted): return ["event": "mute", "muted": muted]
    case .audioActivated(let generation): return ["event": "audioActivated", "generation": generation]
    case .audioDeactivated(let generation): return ["event": "audioDeactivated", "generation": generation]
    case .visible(let visible): return ["event": "visible", "visible": visible]
    case .orientation(let orientation): return ["event": "orientation", "orientation": orientation]
    case .videoRequested: return ["event": "video"]
    case .route(let output): return ["event": "route", "route": output.rawValue]
    }
}

// PushKit can launch the app, and the user answer, before Rust has registered
final class CallEventQueue {
    private let limit: Int
    private var sink: ((CallEvent) -> Void)?
    private var waiting: [CallEvent] = []

    init(limit: Int = 16) {
        self.limit = limit
    }

    func register(_ sink: @escaping (CallEvent) -> Void) {
        self.sink = sink
        let ready = waiting
        waiting = []
        ready.forEach(sink)
    }

    func emit(_ event: CallEvent) {
        if let sink {
            sink(event)
            return
        }
        waiting.append(event)
        if waiting.count > limit { waiting.removeFirst(waiting.count - limit) }
    }

    func forget() {
        waiting = []
    }

    @discardableResult
    func offer(_ event: CallEvent) -> Bool {
        guard let sink else { return false }
        sink(event)
        return true
    }
}

func stopEndsCall(answered: Bool, outgoing: Bool = false) -> Bool { !answered && !outgoing }

enum Connect: Equatable {
    case reportConnected
    case nothing
}

func connectWith(outgoing: Bool) -> Connect {
    outgoing ? .reportConnected : .nothing
}

// same 45 s as RING_LIMIT in the app and the router's call push
let callRingLimit: TimeInterval = 45
let callConnectLimit: TimeInterval = 30

func callOverdue(outgoing: Bool, answered: Bool, connected: Bool, ringingFor: TimeInterval, answeredFor: TimeInterval?) -> CXCallEndedReason? {
    if outgoing || connected { return nil }
    if answered {
        guard let answeredFor, answeredFor >= callConnectLimit else { return nil }
        return .failed
    }
    return ringingFor >= callRingLimit ? .unanswered : nil
}

// Apple DTS says to set this again before every reported or started call (iOS 18.4.1+), or the system may
// never activate the audio session. localizedName is ignored since iOS 14.
func callProviderConfiguration(icon: Data?) -> CXProviderConfiguration {
    let configuration = CXProviderConfiguration()
    configuration.iconTemplateImageData = icon
    configuration.supportsVideo = true
    configuration.maximumCallsPerCallGroup = 1
    configuration.maximumCallGroups = 1
    configuration.supportedHandleTypes = [.generic]
    configuration.includesCallsInRecents = false
    return configuration
}

let callKitIconName = "CallKitIcon"

let callKitIcon: Data? = UIImage(named: callKitIconName)?.pngData()

func endEvent(ours: Bool, outgoing: Bool, connected: Bool) -> CallEvent {
    ours && !outgoing && !connected ? .decline : .end
}

func endReason(live: Bool) -> CXCallEndedReason { live ? .remoteEnded : .unanswered }

func callAudioOptions() -> AVAudioSession.CategoryOptions { [.allowBluetoothHFP, .allowBluetoothA2DP] }

// don't activate it here, CallKit does and reports it in didActivate
func configureCallAudio() {
    try? AVAudioSession.sharedInstance().setCategory(.playAndRecord, mode: .voiceChat, options: callAudioOptions())
}

// own queue, so Rust's handler never runs on main and can call the plugin back without a deadlock
final class CallEvents {
    static let shared = CallEvents()
    private let queue = DispatchQueue(label: "com.flickertalk.calls.events")
    private let events = CallEventQueue()

    func register(_ channel: Channel) {
        queue.async { self.events.register { channel.send(callEventPayload($0)) } }
    }

    func emit(_ event: CallEvent) {
        queue.async { self.events.emit(event) }
    }

    func offer(_ event: CallEvent) {
        queue.async { self.events.offer(event) }
    }

    func forget() {
        queue.async { self.events.forget() }
    }
}

struct CallEventsArgs: Decodable {
    let channel: Channel
}

struct SpeakerArgs: Decodable {
    let on: Bool
}

struct OutgoingArgs: Decodable {
    let name: String
    let video: Bool
}

// Apple wants every VoIP push reported to CallKit at once, so it rings as "FlickerTalk" until the name comes
final class Calls: NSObject, PKPushRegistryDelegate, CXProviderDelegate {
    static let shared = Calls()
    private let registry = PKPushRegistry(queue: .main)
    private let provider: CXProvider
    private let controller = CXCallController(queue: .main)
    let voip = VoipToken()
    private var current: UUID?
    private var answered = false
    private var outgoing = false
    private var live = false
    private var generation: UInt64 = 0
    // a late didDeactivate belongs to this generation, not to a call that began since
    private var sessionGeneration: UInt64 = 0
    private var reactivated = false
    private var speakerChoice: Bool?
    private var prefersSpeaker = false
    private var sessionActive = false
    private var reportedRoute: CallOutput?
    private var sessionRestored = false
    private var ringingSince: Date?
    private var answeredAt: Date?
    private var voiceConnected = false
    private let openAppOnAnswer = OpenAppOnAnswerSetting()
    private let screen = CallScreen()
    private var named = false
    private var quiet: (caller: String, video: Bool)?
    private var quietRings = 0
    var openSlots: Set<Int> {
        get { OpenSlots.kept() }
        set { OpenSlots.keep(Array(newValue)) }
    }

    override init() {
        provider = CXProvider(configuration: callProviderConfiguration(icon: callKitIcon))
        super.init()
        provider.setDelegate(self, queue: .main)
        registry.delegate = self
        registry.desiredPushTypes = [.voIP]
        NotificationCenter.default.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] note in
            self?.interrupted(note)
        }
        NotificationCenter.default.addObserver(forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main) { [weak self] note in
            self?.routeChanged(note)
        }
    }

    // if CallKit doesn't reactivate the session within a second of "should resume", restart the voice here (Signal does the same)
    private func interrupted(_ note: Notification) {
        let type = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt).flatMap(AVAudioSession.InterruptionType.init(rawValue:))
        guard type == .ended else { return }
        let options = (note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt).map(AVAudioSession.InterruptionOptions.init(rawValue:)) ?? []
        reactivated = false
        let call = current
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in
            guard let self else { return }
            let live = call != nil && self.current == call && (self.live || self.outgoing)
            guard restartAfterInterruption(shouldResume: options.contains(.shouldResume), reactivated: self.reactivated, callLive: live) else { return }
            self.applyRoute()
            self.reportRoute()
            CallEvents.shared.emit(.audioDeactivated(self.sessionGeneration))
            CallEvents.shared.emit(.audioActivated(self.sessionGeneration))
        }
    }

    // an override only lasts while the session is active, so it's applied again on each activation
    func setSpeaker(_ on: Bool) {
        speakerChoice = on
        applyRoute()
    }

    // iOS may leave a Bluetooth headset out of a call that starts on the receiver, so prefer the headset
    // mic, which moves the voice there both ways. Dropping the override alone lands on the receiver.
    private func applyRoute() {
        guard current != nil, sessionActive else { return }
        let session = AVAudioSession.sharedInstance()
        if !isCallSession(category: session.category, mode: session.mode) {
            routeLog("the session changed under the call: \(describeSession(session))")
            if !sessionRestored {
                sessionRestored = true
                configureCallAudio()
            }
        }
        let route = session.currentRoute
        let available = session.availableInputs ?? []
        let ports = available.map(\.portType)
        let speaker = speakerOn(choice: speakerChoice, prefersSpeaker: prefersSpeaker, headset: hasHeadset(outputs: route.outputs.map(\.portType), available: ports))
        if !speaker {
            let port = headsetInput(inRoute: route.inputs.map(\.portType), available: ports)
            let input = port.flatMap { port in available.first { $0.portType == port } }
            if input != nil || session.preferredInput != nil {
                try? session.setPreferredInput(input)
            }
        }
        try? session.overrideOutputAudioPort(outputOverride(speaker: speaker))
        routeLog("apply speaker=\(speaker) choice=\(speakerChoice.map { "\($0)" } ?? "none") video=\(prefersSpeaker) \(describeSession(session))")
    }

    private func reportRoute() {
        guard current != nil, sessionActive else { return }
        let output = callOutput(of: AVAudioSession.sharedInstance().currentRoute.outputs.map(\.portType))
        guard output != reportedRoute else { return }
        reportedRoute = output
        CallEvents.shared.emit(.route(output))
    }

    private func routeChanged(_ note: Notification) {
        guard current != nil, sessionActive else { return }
        let reason = (note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt).flatMap(AVAudioSession.RouteChangeReason.init(rawValue:)) ?? .unknown
        routeLog("change reason=\(reason.rawValue) \(describeSession(AVAudioSession.sharedInstance()))")
        speakerChoice = choiceAfterRouteChange(speakerChoice, reason: reason)
        if reroutes(after: reason) { applyRoute() }
        reportRoute()
    }

    func requestAnswer() -> Bool {
        let path = appAnswer(callKitCall: current != nil, answered: answered, outgoing: outgoing, quietRinging: quiet != nil)
        quietLog("answer in the app: \(path)")
        if path == .join, let ringing = quiet {
            // a quiet call CallKit never rang joins it as a started call, whose start action sets up the audio
            stopQuiet()
            startOutgoing(name: ringing.caller, video: ringing.video) { _ in }
            return false
        }
        guard path == .callKit, let call = current else { return false }
        controller.request(CXTransaction(action: CXAnswerCallAction(call: call))) { error in
            guard error != nil else { return }
            CallEvents.shared.emit(.answer)
        }
        return true
    }

    private func configureProvider() {
        provider.configuration = callProviderConfiguration(icon: callKitIcon)
    }

    private func watch(_ uuid: UUID, after seconds: TimeInterval) {
        DispatchQueue.main.asyncAfter(wallDeadline: .now() + seconds) { [weak self] in
            self?.endIfOverdue(uuid)
        }
    }

    private func endIfOverdue(_ uuid: UUID) {
        guard current == uuid else { return }
        let now = Date()
        let overdue = callOverdue(
            outgoing: outgoing,
            answered: answered,
            connected: voiceConnected,
            ringingFor: ringingSince.map { now.timeIntervalSince($0) } ?? 0,
            answeredFor: answeredAt.map { now.timeIntervalSince($0) }
        )
        guard let reason = overdue else { return }
        finish()
        provider.reportCall(with: uuid, endedAt: nil, reason: reason)
        CallEvents.shared.emit(.end)
    }

    private func update(caller: String, video: Bool) -> CXCallUpdate {
        callUpdate(caller: caller, video: video, outgoing: outgoing, openAppOnAnswer: openAppOnAnswer.on)
    }

    func setOpenAppOnAnswer(_ on: Bool) {
        openAppOnAnswer.on = on
    }

    private func begin(_ uuid: UUID, outgoing: Bool) {
        current = uuid
        answered = false
        live = false
        voiceConnected = false
        ringingSince = nil
        answeredAt = nil
        speakerChoice = nil
        prefersSpeaker = false
        reportedRoute = nil
        sessionRestored = false
        named = false
        generation += 1
        self.outgoing = outgoing
        // what the user did for an earlier call must not answer or hang up this one
        CallEvents.shared.forget()
    }

    private func finish() {
        if current != nil { AppVisibility.holdAfterCall() }
        current = nil
        named = false
        answered = false
        live = false
        outgoing = false
        voiceConnected = false
        ringingSince = nil
        answeredAt = nil
    }

    private func report(caller: String, video: Bool, done: (() -> Void)? = nil) {
        let uuid = UUID()
        begin(uuid, outgoing: false)
        ringingSince = Date()
        watch(uuid, after: callRingLimit)
        configureProvider()
        let shown = update(caller: caller, video: video)
        provider.reportNewIncomingCall(with: uuid, update: shown) { [weak self] error in
            if error != nil {
                if self?.current == uuid { self?.current = nil }
            }
            done?()
        }
    }

    func ring(caller: String, video: Bool, quiet: Bool) {
        let ring = ringWith(callKitCall: current != nil, answered: answered, quiet: quiet)
        quietLog("ring: \(ring)")
        switch ring {
        case .report:
            report(caller: caller, video: video)
            named = true
        case .update:
            if let current { provider.reportCall(with: current, updated: update(caller: caller, video: video)) }
            named = true
        case .silent:
            ringQuietly(caller: caller, video: video)
        case .endAndSilent:
            endWithoutTrace()
            ringQuietly(caller: caller, video: video)
        }
    }

    private func ringQuietly(caller: String, video: Bool) {
        quiet = (caller, video)
        quietRings += 1
        let ringing = quietRings
        DispatchQueue.main.asyncAfter(wallDeadline: .now() + callRingLimit) { [weak self] in
            if self?.quietRings == ringing { self?.stopQuiet() }
        }
        guard showsQuietNotice(appActive: UIApplication.shared.applicationState == .active) else { return }
        let request = UNNotificationRequest(identifier: quietCallIdentifier, content: quietCallContent(caller: caller, video: video), trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    private func stopQuiet() {
        guard quiet != nil else { return }
        quiet = nil
        let center = UNUserNotificationCenter.current()
        center.removePendingNotificationRequests(withIdentifiers: [quietCallIdentifier])
        center.removeDeliveredNotifications(withIdentifiers: [quietCallIdentifier])
    }

    private func endWithoutTrace() {
        guard let call = current else { return }
        finish()
        provider.reportCall(with: call, endedAt: nil, reason: noTraceEndReason)
    }

    func refused() {
        let ends = refusalEndsCall(callKitCall: current != nil, outgoing: outgoing, named: named)
        quietLog(ends ? "refused: the push's call ended" : "refused: nothing to end")
        if ends { endWithoutTrace() }
    }

    func answering(caller: String, video: Bool) {
        guard let current else { return }
        named = true
        if video { preferSpeaker() }
        provider.reportCall(with: current, updated: answeringUpdate(caller: caller, video: video, openAppOnAnswer: openAppOnAnswer.on))
    }

    func stop() {
        stopQuiet()
        guard let call = current, stopEndsCall(answered: answered, outgoing: outgoing) else { return }
        finish()
        provider.reportCall(with: call, endedAt: nil, reason: .remoteEnded)
    }

    func startOutgoing(name: String, video: Bool, done: @escaping (Error?) -> Void) {
        if let old = current { provider.reportCall(with: old, endedAt: nil, reason: .remoteEnded) }
        let uuid = UUID()
        begin(uuid, outgoing: true)
        configureProvider()
        let shown = name.isEmpty ? "FlickerTalk" : name
        if video { preferSpeaker() }
        let action = CXStartCallAction(call: uuid, handle: CXHandle(type: .generic, value: shown))
        action.isVideo = video
        controller.request(CXTransaction(action: action)) { [weak self] error in
            guard let self else { return }
            if error != nil {
                if self.current == uuid { self.finish() }
            } else if self.current == uuid {
                self.provider.reportCall(with: uuid, updated: self.update(caller: shown, video: video))
                self.screen.callIs(.started)
            }
            done(error)
        }
    }

    func connected() {
        voiceConnected = true
        screen.callIs(.connected)
        switch connectWith(outgoing: outgoing) {
        case .reportConnected:
            live = true
            if let current { provider.reportOutgoingCall(with: current, connectedAt: nil) }
        case .nothing:
            break
        }
    }

    // hasVideo, so unlocking the phone opens the app on the call
    func setVideo(_ on: Bool) {
        guard let current else { return }
        provider.reportCall(with: current, updated: videoUpdate(on, outgoing: outgoing, openAppOnAnswer: openAppOnAnswer.on))
        if on { preferSpeaker() }
    }

    private func preferSpeaker() {
        guard !prefersSpeaker else { return }
        prefersSpeaker = true
        applyRoute()
    }

    func ended() {
        stopQuiet()
        screen.callIs(.ended)
        guard let call = current else {
            finish()
            return
        }
        let reason = endReason(live: live || answered)
        finish()
        provider.reportCall(with: call, endedAt: nil, reason: reason)
    }

    func pushRegistry(_ registry: PKPushRegistry, didUpdate credentials: PKPushCredentials, for type: PKPushType) {
        voip.update(credentials.token)
    }

    func pushRegistry(_ registry: PKPushRegistry, didInvalidatePushTokenFor type: PKPushType) {
        voip.update(nil)
    }

    func pushRegistry(_ registry: PKPushRegistry, didReceiveIncomingPushWith payload: PKPushPayload, for type: PKPushType, completion: @escaping () -> Void) {
        // Every VoIP push must ring, or iOS stops delivering them. One that mustn't ring is reported and
        // ended in the same turn. The core reconnects after report, whose new call clears older events.
        let push = pushCall(
            callKitCall: current != nil,
            slotHeard: slotIsHeard(payload.dictionaryPayload["s"], open: openSlots),
            mayDisturb: QuietHours.mayDisturbNow()
        )
        quietLog("call push: \(push.logName)")
        switch push {
        case .alreadyRinging:
            CallEvents.shared.emit(.incoming)
            completion()
        case .ring:
            report(caller: "", video: false, done: completion)
            CallEvents.shared.emit(.incoming)
        case .endAtOnce:
            reportEndedAtOnce(done: completion)
            CallEvents.shared.emit(.incoming)
            holdForCore()
        }
    }

    // Apple kills an app that doesn't report a VoIP push. End it again once CallKit has it, in case the
    // first end came too early.
    private func reportEndedAtOnce(done: @escaping () -> Void) {
        let uuid = UUID()
        configureProvider()
        provider.reportNewIncomingCall(with: uuid, update: update(caller: "", video: false)) { [weak self] _ in
            self?.provider.reportCall(with: uuid, endedAt: nil, reason: noTraceEndReason)
            done()
        }
        provider.reportCall(with: uuid, endedAt: nil, reason: noTraceEndReason)
    }

    // with no CallKit call iOS may suspend the app at once, give the core a moment to connect
    private func holdForCore() {
        var task = UIBackgroundTaskIdentifier.invalid
        let end = {
            guard task != .invalid else { return }
            UIApplication.shared.endBackgroundTask(task)
            task = .invalid
        }
        task = UIApplication.shared.beginBackgroundTask(withName: "ft.call.quiet", expirationHandler: end)
        DispatchQueue.main.asyncAfter(deadline: .now() + 25, execute: end)
    }

    func providerDidReset(_ provider: CXProvider) {
        if current != nil { CallEvents.shared.emit(.end) }
        finish()
    }

    func provider(_ provider: CXProvider, perform action: CXStartCallAction) {
        configureCallAudio()
        provider.reportOutgoingCall(with: action.callUUID, startedConnectingAt: nil)
        action.fulfill()
    }

    // fulfil at once, Apple says waiting for the voice loses the audio
    func provider(_ provider: CXProvider, perform action: CXAnswerCallAction) {
        configureCallAudio()
        answeredAt = Date()
        watch(action.callUUID, after: callConnectLimit)
        answered = true
        live = true
        CallEvents.shared.emit(.answer)
        action.fulfill()
    }

    func provider(_ provider: CXProvider, perform action: CXEndCallAction) {
        let event = endEvent(ours: current == action.callUUID, outgoing: outgoing, connected: voiceConnected)
        if current == action.callUUID { finish() }
        CallEvents.shared.emit(event)
        action.fulfill()
    }

    func provider(_ provider: CXProvider, perform action: CXSetMutedCallAction) {
        CallEvents.shared.emit(.mute(action.isMuted))
        action.fulfill()
    }

    // Rust starts its audio unit only once the system activates the session
    func provider(_ provider: CXProvider, didActivate audioSession: AVAudioSession) {
        sessionGeneration = generation
        reactivated = true
        sessionActive = true
        routeLog("activated \(describeSession(audioSession))")
        applyRoute()
        reportRoute()
        CallEvents.shared.emit(.audioActivated(sessionGeneration))
    }

    func provider(_ provider: CXProvider, didDeactivate audioSession: AVAudioSession) {
        sessionActive = false
        CallEvents.shared.emit(.audioDeactivated(sessionGeneration))
    }
}

func isWakePush(_ userInfo: [AnyHashable: Any]) -> Bool {
    (userInfo["t"] as? String) == "wake"
}

// registered at load, a VoIP push may be what launched the app
private let callsAtLaunch = Calls.shared

// Tauri owns the app delegate, so the token callbacks are added to its class
final class RemoteToken {
    static let shared = RemoteToken()
    private var waiting: [Invoke] = []
    private var installed = false

    func ask(_ invoke: Invoke) {
        waiting.append(invoke)
        install()
        UIApplication.shared.registerForRemoteNotifications()
    }

    private func install() {
        guard !installed, let delegate = UIApplication.shared.delegate else { return }
        installed = true
        let cls: AnyClass = type(of: delegate)
        let registered: @convention(block) (AnyObject, UIApplication, Data) -> Void = { _, _, token in
            RemoteToken.shared.answer(token: token, error: nil)
        }
        let failed: @convention(block) (AnyObject, UIApplication, NSError) -> Void = { _, _, error in
            RemoteToken.shared.answer(token: nil, error: error)
        }
        class_addMethod(cls, #selector(UIApplicationDelegate.application(_:didRegisterForRemoteNotificationsWithDeviceToken:)), imp_implementationWithBlock(registered), "v@:@@")
        class_addMethod(cls, #selector(UIApplicationDelegate.application(_:didFailToRegisterForRemoteNotificationsWithError:)), imp_implementationWithBlock(failed), "v@:@@")
    }

    private func answer(token: Data?, error: Error?) {
        let invokes = waiting
        waiting = []
        guard let token else {
            invokes.forEach { $0.reject(error?.localizedDescription ?? "no push token") }
            return
        }
        // PushKit's token often comes after Apple's, without it a call only shows a notification
        Calls.shared.voip.when(within: 5) { voip in
            let profile = Bundle.main.path(forResource: "embedded", ofType: "mobileprovision")
                .flatMap { FileManager.default.contents(atPath: $0) }
                .map { String(decoding: $0, as: UTF8.self) }
            let bundle = Bundle.main.bundleIdentifier ?? ""
            let target = pushTarget(gateway: apnsGateway(provisioning: profile), bundle: bundle, token: token, voip: voip)
            invokes.forEach { $0.resolve(["token": target]) }
        }
    }
}

final class VoipToken {
    private(set) var token: Data?
    private var waiting: [(Data?) -> Void] = []

    func update(_ token: Data?) {
        self.token = token
        guard let token else { return }
        let ready = waiting
        waiting = []
        ready.forEach { $0(token) }
    }

    func when(within seconds: TimeInterval, _ then: @escaping (Data?) -> Void) {
        if let token {
            then(token)
            return
        }
        var answered = false
        let once: (Data?) -> Void = { token in
            guard !answered else { return }
            answered = true
            then(token)
        }
        waiting.append(once)
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds) { once(nil) }
    }
}

struct AnsweringArgs: Decodable {
    let caller: String
    let video: Bool
}

struct RingingArgs: Decodable {
    let caller: String
    let video: Bool
    let muted: Bool
}

struct OpenFileArgs: Decodable {
    let path: String
    let mime: String
}

final class PreviewItem: NSObject, QLPreviewControllerDataSource, QLPreviewItem {
    let previewItemURL: URL?
    let previewItemTitle: String?

    init(url: URL) {
        previewItemURL = url
        previewItemTitle = url.lastPathComponent
    }

    func numberOfPreviewItems(in controller: QLPreviewController) -> Int { 1 }

    func previewController(_ controller: QLPreviewController, previewItemAt index: Int) -> QLPreviewItem { self }
}

func previewable(path: String) -> PreviewItem? {
    guard !path.isEmpty, FileManager.default.fileExists(atPath: path) else { return nil }
    let item = PreviewItem(url: URL(fileURLWithPath: path))
    return QLPreviewController.canPreview(item) ? item : nil
}

class PlatformPlugin: Plugin {
    private var authSession: ASWebAuthenticationSession?
    private let authAnchor = AuthAnchor()
    private weak var webView: WKWebView?

    override init() {
        super.init()
        UNUserNotificationCenter.current().delegate = ReminderTaps.shared
        _ = callsAtLaunch
        StoreUpdates.shared.start()
        let install = {
            AppVisibility.install()
            CallIntentActivities.install()
        }
        if Thread.isMainThread { install() } else { DispatchQueue.main.async(execute: install) }
    }

    override func load(webview: WKWebView) {
        webView = webview
    }

    @objc public func startRinging(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(RingingArgs.self)
        DispatchQueue.main.async {
            let quiet = quietCall(muted: args.muted, mayDisturb: QuietHours.mayDisturbNow())
            Calls.shared.ring(caller: args.caller, video: args.video, quiet: quiet)
            invoke.resolve()
        }
    }

    @objc public func callRefused(_ invoke: Invoke) throws {
        DispatchQueue.main.async {
            Calls.shared.refused()
            invoke.resolve()
        }
    }

    @objc public func setOpenSlots(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(OpenSlotsArgs.self)
        DispatchQueue.main.async {
            Calls.shared.openSlots = Set(args.slots)
            invoke.resolve()
        }
    }

    @objc public func setQuietHours(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(QuietHoursArgs.self)
        QuietHours.keep(args.week)
        invoke.resolve()
    }

    @objc public func stopRinging(_ invoke: Invoke) throws {
        DispatchQueue.main.async {
            Calls.shared.stop()
            invoke.resolve()
        }
    }

    @objc public func registerCallEvents(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(CallEventsArgs.self)
        CallEvents.shared.register(args.channel)
        DispatchQueue.main.async { AppVisibility.tellNow() }
        invoke.resolve()
    }

    @objc public func attachVideo(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(AttachVideoArgs.self)
        DispatchQueue.main.async { [weak self] in
            guard let web = self?.webView else {
                invoke.reject("no webview")
                return
            }
            CallVideoViews.shared.attach(webView: web, remote: rustLayer(args.remoteLayer), local: rustLayer(args.localLayer))
            CallVideoViews.shared.startOrientation()
            invoke.resolve()
        }
    }

    @objc public func videoLayout(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(VideoLayoutArgs.self)
        DispatchQueue.main.async {
            CallVideoViews.shared.layout(args)
            invoke.resolve()
        }
    }

    @objc public func videoShape(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(VideoShapeArgs.self)
        DispatchQueue.main.async {
            CallVideoViews.shared.shape(rotation: args.rotation)
            invoke.resolve()
        }
    }

    @objc public func detachVideo(_ invoke: Invoke) throws {
        DispatchQueue.main.async {
            CallVideoViews.shared.detach()
            invoke.resolve()
        }
    }

    @objc public func setOpenAppOnAnswer(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(OpenAppOnAnswerArgs.self)
        DispatchQueue.main.async {
            Calls.shared.setOpenAppOnAnswer(args.on)
            invoke.resolve()
        }
    }

    @objc public func callVideo(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(CallVideoArgs.self)
        DispatchQueue.main.async {
            Calls.shared.setVideo(args.on)
            invoke.resolve()
        }
    }

    @objc public func requestCamera(_ invoke: Invoke) throws {
        switch cameraAccess(AVCaptureDevice.authorizationStatus(for: .video)) {
        case .granted:
            invoke.resolve(["granted": true])
        case .denied:
            invoke.resolve(["granted": false])
        case .ask:
            AVCaptureDevice.requestAccess(for: .video) { invoke.resolve(["granted": $0]) }
        }
    }

    @objc public func currentLocation(_ invoke: Invoke) throws {
        DispatchQueue.main.async {
            LocationRequest.start { payload in invoke.resolve(payload.mapValues { $0 as Any? }) }
        }
    }

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

    @objc public func callStartedOutgoing(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(OutgoingArgs.self)
        DispatchQueue.main.async {
            Calls.shared.startOutgoing(name: args.name, video: args.video) { error in
                if let error {
                    invoke.reject("callkit_refused: \(error.localizedDescription)")
                } else {
                    invoke.resolve()
                }
            }
        }
    }

    @objc public func callConnected(_ invoke: Invoke) throws {
        DispatchQueue.main.async {
            Calls.shared.connected()
            invoke.resolve()
        }
    }

    @objc public func answerCall(_ invoke: Invoke) throws {
        DispatchQueue.main.async {
            invoke.resolve(["answered": Calls.shared.requestAnswer()])
        }
    }

    @objc public func setSpeaker(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(SpeakerArgs.self)
        DispatchQueue.main.async {
            Calls.shared.setSpeaker(args.on)
            invoke.resolve()
        }
    }

    @objc public func callEnded(_ invoke: Invoke) throws {
        DispatchQueue.main.async {
            Calls.shared.ended()
            invoke.resolve()
        }
    }

    @objc public func requestMicrophone(_ invoke: Invoke) throws {
        if #available(iOS 17.0, *) {
            AVAudioApplication.requestRecordPermission { invoke.resolve(["granted": $0]) }
        } else {
            AVAudioSession.sharedInstance().requestRecordPermission { invoke.resolve(["granted": $0]) }
        }
    }

    @objc public func callAnswering(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(AnsweringArgs.self)
        DispatchQueue.main.async {
            Calls.shared.answering(caller: args.caller, video: args.video)
            invoke.resolve()
        }
    }

    @objc public func requestNotifications(_ invoke: Invoke) throws {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in
            invoke.resolve()
        }
    }

    // the router expects gateway:bundle:hex[:voip-hex]
    @objc public func pushToken(_ invoke: Invoke) throws {
        DispatchQueue.main.async { RemoteToken.shared.ask(invoke) }
    }

    @objc public func pendingReminder(_ invoke: Invoke) throws {
        invoke.resolve(["reminder": ReminderTaps.shared.pending])
        ReminderTaps.shared.pending = ""
    }

    // no note text unless the core sent one
    @objc public func setReminders(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(RemindersArgs.self)
        let entries = parseReminders(args.reminders)
        let center = UNUserNotificationCenter.current()
        center.getPendingNotificationRequests { requests in
            let ours = requests.map(\.identifier).filter { $0.hasPrefix("ft.reminder|") }
            center.removePendingNotificationRequests(withIdentifiers: ours)
            let now = Int64(Date().timeIntervalSince1970 * 1000)
            for entry in remindersToSchedule(entries, now: now) {
                let content = UNMutableNotificationContent()
                content.title = NSLocalizedString("Reminder", comment: "")
                content.body = (entry.text ?? "").isEmpty ? NSLocalizedString("You have a reminder", comment: "") : entry.text!
                content.sound = .default
                content.categoryIdentifier = "ft.reminder"
                let date = Date(timeIntervalSince1970: TimeInterval(entry.at) / 1000)
                let parts = Calendar.current.dateComponents([.year, .month, .day, .hour, .minute, .second], from: date)
                let trigger = UNCalendarNotificationTrigger(dateMatching: parts, repeats: false)
                center.add(UNNotificationRequest(identifier: reminderIdentifier(entry), content: content, trigger: trigger))
            }
            invoke.resolve()
        }
    }

    // the WebView never sees the login
    @objc public func authorize(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(AuthorizeArgs.self)
        guard let url = URL(string: args.url) else {
            invoke.reject("not a login page")
            return
        }
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            let session = ASWebAuthenticationSession(url: url, callbackURLScheme: args.scheme) { callback, error in
                if let callback, isAuthRedirect(callback, scheme: args.scheme) {
                    invoke.resolve(["url": callback.absoluteString])
                } else {
                    invoke.reject(error?.localizedDescription ?? "cancelled")
                }
                self.authSession = nil
            }
            session.presentationContextProvider = self.authAnchor
            session.prefersEphemeralWebBrowserSession = false
            self.authSession = session
            session.start()
        }
    }

    @objc public func openFile(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(OpenFileArgs.self)
        guard let item = previewable(path: args.path) else {
            invoke.reject("that file cannot be shown here")
            return
        }
        DispatchQueue.main.async { [manager] in
            guard let screen = manager.viewController else {
                invoke.reject("no screen to show it on")
                return
            }
            let preview = QLPreviewController()
            preview.dataSource = item
            // QLPreviewController holds its data source weakly
            objc_setAssociatedObject(preview, "ft.preview.item", item, .OBJC_ASSOCIATION_RETAIN)
            screen.present(preview, animated: true)
            invoke.resolve()
        }
    }

    @objc public func sealKey(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(KeyArgs.self)
        guard let key = Data(base64Encoded: args.value), key.count == 32 else {
            invoke.reject("not a storage key")
            return
        }
        do {
            try storeKey(key)
            invoke.resolve(["value": keychainMarker().base64EncodedString()])
        } catch {
            invoke.reject("the Keychain refused the key: \(error)")
        }
    }

    @objc public func shareText(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(ShareArgs.self)
        guard let text = shareableText(args.text) else {
            invoke.reject("nothing to share")
            return
        }
        DispatchQueue.main.async { [manager] in
            guard let screen = manager.viewController else {
                invoke.reject("no screen to share from")
                return
            }
            let sheet = UIActivityViewController(activityItems: [text], applicationActivities: nil)
            // on iPad the sheet is a popover and needs an anchor
            UIUtils.centerPopover(rootViewController: screen, popoverController: sheet)
            screen.present(sheet, animated: true)
            invoke.resolve()
        }
    }

    @objc public func openKey(_ invoke: Invoke) throws {
        do {
            invoke.resolve(["value": try loadKey().base64EncodedString()])
        } catch {
            invoke.reject("the Keychain has no storage key: \(error)")
        }
    }

    // rejections are plan.trouble.* keys, the app translates them
    private func entitlements() async -> [StoreEntitlement] {
        await currentEntitlements()
    }

    @objc public func openedLink(_ invoke: Invoke) throws {
        invoke.resolve(["url": OpenedLinks.shared.take()])
    }

    @objc public func registerLinkEvents(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(CallEventsArgs.self)
        OpenedLinks.shared.register { args.channel.send(["event": "linkOpened"] as JsonObject) }
        invoke.resolve()
    }

    @objc public func registerEntitlementEvents(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(CallEventsArgs.self)
        StoreUpdates.shared.register(args.channel)
        invoke.resolve()
    }

    @objc public func subscription(_ invoke: Invoke) throws {
        Task {
            invoke.resolve(["until": activeUntil(await entitlements(), now: Date())])
        }
    }

    @objc public func restoreSubscription(_ invoke: Invoke) throws {
        Task {
            do {
                try await AppStore.sync()
                invoke.resolve(["until": activeUntil(await entitlements(), now: Date())])
            } catch {
                invoke.reject(restoreTrouble(error))
            }
        }
    }

    @objc public func subscriptionPrice(_ invoke: Invoke) throws {
        Task {
            let products = (try? await Product.products(for: [yearly])) ?? []
            let price = yearlyPrice(products.map { StoreProduct(id: $0.id, displayPrice: $0.displayPrice) })
            if let price {
                invoke.resolve(["price": price])
            } else {
                invoke.resolve()
            }
        }
    }

    @objc public func subscribe(_ invoke: Invoke) throws {
        Task {
            do {
                guard let product = try await Product.products(for: [yearly]).first else {
                    invoke.reject("not_on_sale")
                    return
                }
                switch try await product.purchase() {
                case .success(let signed):
                    guard case .verified(let transaction) = signed else {
                        invoke.reject("payment_failed")
                        return
                    }
                    await transaction.finish()
                    invoke.resolve(["until": activeUntil(await entitlements(), now: Date())])
                case .userCancelled:
                    invoke.reject("cancelled")
                case .pending:
                    // Ask to Buy isn't paid for yet, don't pretend it is
                    invoke.reject("pending_approval")
                @unknown default:
                    invoke.reject("payment_failed")
                }
            } catch {
                invoke.reject("payment_failed")
            }
        }
    }
}

final class AuthAnchor: NSObject, ASWebAuthenticationPresentationContextProviding {
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows)
            .first { $0.isKeyWindow } ?? ASPresentationAnchor()
    }
}

@_cdecl("init_plugin_ft_platform")
func initPlugin() -> Plugin {
    return PlatformPlugin()
}
