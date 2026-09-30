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

// FlickerTalk's native bridge on iOS (see ../../src/lib.rs): the storage key in the Keychain on
// this device only (Plan §94), the share sheet, APNs and PushKit, and calls through CallKit.

private let keyService = "com.flickertalk.app.storage"
private let keyAccount = "storage-key"

/// What Rust keeps in `storage.key.sealed` on iOS: only the name of the Keychain item.
func keychainMarker() -> Data {
    Data("keychain:v1".utf8)
}

/// Readable after the first unlock, and never synced to iCloud or restored on another device.
func keyAccessibility() -> String {
    kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly as String
}

/// The text for the share sheet, or nil when there is nothing to share.
func shareableText(_ text: String) -> String? {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}

/// The yearly subscription, as it is named in App Store Connect (§40-42).
let yearly = "yearly"

/// One entitlement as the Store handed it back, with none of StoreKit's types in it.
struct StoreEntitlement {
    let product: String
    let expires: Date?
    let revoked: Date?
}

/// Until when this phone is paid up, in milliseconds as the core counts time; 0 when nothing is.
/// StoreKit says when a subscription runs out, so unlike Play nothing is guessed here (§45).
func activeUntil(_ entitlements: [StoreEntitlement], now: Date) -> Int64 {
    entitlements
        .filter { $0.product == yearly && $0.revoked == nil }
        .compactMap { $0.expires }
        .filter { $0 > now }
        .map { Int64(($0.timeIntervalSince1970 * 1000).rounded()) }
        .max() ?? 0
}

/// One product as the Store handed it back, with none of StoreKit's types in it.
struct StoreProduct {
    let id: String
    let displayPrice: String
}

/// What a year costs, exactly as the Store formats it for this Apple ID's storefront, or nil when
/// it cannot say (2026-09-29). Nothing is converted, rounded or kept.
func yearlyPrice(_ products: [StoreProduct]) -> String? {
    guard let price = products.first(where: { $0.id == yearly })?.displayPrice
        .trimmingCharacters(in: .whitespacesAndNewlines), !price.isEmpty else { return nil }
    return price
}

/// One reminder as the core wrote it (2026-09-27).
struct ReminderEntry: Decodable, Equatable {
    let plugin: String
    let id: String
    let at: Int64
    let text: String?
}

/// What the core sends: `[{plugin, id, at, text}]`. Anything unreadable is nothing, not a crash.
func parseReminders(_ json: String) -> [ReminderEntry] {
    guard let data = json.data(using: .utf8),
          let entries = try? JSONDecoder().decode([ReminderEntry].self, from: data) else { return [] }
    return entries.filter { !$0.plugin.isEmpty && !$0.id.isEmpty && $0.at > 0 }
}

/// The notification's identifier: one per reminder, so the same reminder replaces its own.
func reminderIdentifier(_ entry: ReminderEntry) -> String { "ft.reminder|" + entry.plugin + "|" + entry.id }

/// `plugin\nid`, what the app is opened with when a reminder is tapped.
func reminderKey(_ identifier: String) -> String? {
    let parts = identifier.split(separator: "|", maxSplits: 2).map(String.init)
    guard parts.count == 3, parts[0] == "ft.reminder" else { return nil }
    return parts[1] + "\n" + parts[2]
}

/// iOS keeps at most 64 pending notifications per app: the soonest ones are set, the rest wait
/// for the next start.
func remindersToSchedule(_ entries: [ReminderEntry], now: Int64, limit: Int = 64) -> [ReminderEntry] {
    Array(entries.filter { $0.at > now }.sorted { $0.at < $1.at }.prefix(limit))
}

/// Whether a login's redirect is the one we wait for: our scheme, not some other link.
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

/// Hears the tap on a reminder notification and keeps it until the app asks (2026-09-27).
final class ReminderTaps: NSObject, UNUserNotificationCenterDelegate {
    static let shared = ReminderTaps()
    var pending: String = ""

    // iOS may call the delegate off the main thread, and UIKit wants the answer on it: the `async`
    // forms answered from elsewhere and a tapped notification stopped the app (2026-09-28).
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
        // Our wake-up only matters when the app is not on the screen: here it already connects.
        let options: UNNotificationPresentationOptions = isWakePush(userInfo) ? [] : [.banner, .sound, .list]
        DispatchQueue.main.async { done(options) }
    }
}

// ---- APNs (2026-09-28) ----

/// Which APNs gateway this build's token is for: a build signed for development (Xcode) carries
/// a provisioning profile that says `aps-environment` `development`; an App Store build carries
/// none. Pure, for the tests.
func apnsGateway(provisioning: String?) -> String {
    guard let profile = provisioning else { return "production" }
    let development = "<key>aps-environment</key>\\s*<string>development</string>"
    return profile.range(of: development, options: .regularExpression) != nil ? "sandbox" : "production"
}

private func hex(_ data: Data) -> String {
    data.map { String(format: "%02x", $0) }.joined()
}

/// What the router is given: the gateway, the app and Apple's token, in hex, and PushKit's when
/// there is one, so a call can ring through CallKit (2026-09-28).
func pushTarget(gateway: String, bundle: String, token: Data, voip: Data? = nil) -> String {
    let target = "\(gateway):\(bundle):\(hex(token))"
    return voip.map { "\(target):\(hex($0))" } ?? target
}

/// Whether a push is our router's call (`t: call`), which rings through CallKit.
func isCallPush(_ userInfo: [AnyHashable: Any]) -> Bool {
    (userInfo["t"] as? String) == "call"
}

/// How an incoming call rings (2026-09-28): always through CallKit, once per call, also with the
/// app on the screen (iOS shows its banner), so that answering in the app or on the lock screen
/// takes the same path; a call PushKit already reported only learns who it is.
enum Ring: Equatable {
    case report
    case update
}

func ringWith(callKitCall: Bool) -> Ring {
    callKitCall ? .update : .report
}

/// Whether the app's own answer button asks CallKit to answer (a `CXAnswerCallAction`): the audio
/// session then comes as on the lock screen. Without a CallKit call the core answers by itself.
func answerThroughCallKit(callKitCall: Bool, answered: Bool, outgoing: Bool) -> Bool {
    callKitCall && !answered && !outgoing
}

/// The speaker, or the receiver (`.none`) that `.voiceChat` plays on by default.
func outputOverride(speaker: Bool) -> AVAudioSession.PortOverride {
    speaker ? .speaker : .none
}

/// After an interruption ends with "should resume", whether the voice is started again here:
/// CallKit did not activate the session again, and the call is live.
func restartAfterInterruption(shouldResume: Bool, reactivated: Bool, callLive: Bool) -> Bool {
    shouldResume && !reactivated && callLive
}

// ---- Native calls (2026-09-28) ----
// The call's media lives in Rust now, so CallKit owns the audio session in both directions and
// tells the core, through a channel made in Rust, what the user does. No WebView in between: a
// call answered on a locked iPhone that PushKit woke has audio.

/// What CallKit tells the core, as Rust's `NativeCallEvent`.
enum CallEvent: Equatable {
    /// A call push: the router found this phone offline, so the core reconnects at once.
    case incoming
    case answer
    case end
    /// CallKit's end of an incoming call whose voice never connected (2026-09-29): before the
    /// offer came, the core waits for it and declines it as it arrives, instead of ringing again.
    case decline
    case mute(Bool)
    /// With the generation of the CallKit call (it grows with each call): Rust tells a late event
    /// of an older call apart.
    case audioActivated(UInt64)
    case audioDeactivated(UInt64)
    /// Native video (2026-09-29): the app came to the screen or left it; the core holds our
    /// camera while it is away (iOS stops it anyway).
    case visible(Bool)
    /// The phone turned: `UIDeviceOrientation.rawValue`, for the rotation our frames carry.
    case orientation(Int)
    /// CallKit's "Video" button opened the app with a video call intent: our camera turns on.
    case videoRequested
}

/// The event as the channel carries it: `{"event": "mute", "muted": true}`.
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
    }
}

/// Events wait here until the core listens (PushKit may launch the app, and the user answer on
/// the lock screen, before Rust has registered), then go out in order.
/// Not thread-safe: `CallEvents` uses it on one serial queue.
final class CallEventQueue {
    private let limit: Int
    private var sink: ((CallEvent) -> Void)?
    private var waiting: [CallEvent] = []

    init(limit: Int = 16) {
        self.limit = limit
    }

    /// The core listens: what waited goes out now, in order; a previous listener hears no more.
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

    /// A new call starts: what an old one left unheard is no longer true.
    func forget() {
        waiting = []
    }

    /// Only to a core that listens now; nothing waits (native video: visibility and orientation
    /// are told again when the core registers or the video is attached).
    @discardableResult
    func offer(_ event: CallEvent) -> Bool {
        guard let sink else { return false }
        sink(event)
        return true
    }
}

/// Whether the core's "stop ringing" ends the CallKit call: only an incoming call that still rings.
/// An answered call goes on (CallKit holds its audio) until `callEnded`, and so does this phone's
/// own call.
func stopEndsCall(answered: Bool, outgoing: Bool = false) -> Bool { !answered && !outgoing }

/// What `callConnected` does with CallKit.
enum Connect: Equatable {
    case reportConnected
    case nothing
}

func connectWith(outgoing: Bool) -> Connect {
    outgoing ? .reportConnected : .nothing
}

/// An incoming CallKit call nobody answers ends by itself after this long (s): the caller's limit
/// (`RING_LIMIT` in the app) and the life of the router's call push.
let callRingLimit: TimeInterval = 45
/// An answered CallKit call whose voice never connects ends as failed after this long (s): the
/// offer never came, or the connection never opened.
let callConnectLimit: TimeInterval = 30

/// Whether an incoming CallKit call went nowhere, and how it ends (2026-09-28): if the caller's
/// end never arrives (the phone was out of reach), CallKit must not ring for ever. This phone's
/// own call is the core's to end.
func callOverdue(outgoing: Bool, answered: Bool, connected: Bool, ringingFor: TimeInterval, answeredFor: TimeInterval?) -> CXCallEndedReason? {
    if outgoing || connected { return nil }
    if answered {
        guard let answeredFor, answeredFor >= callConnectLimit else { return nil }
        return .failed
    }
    return ringingFor >= callRingLimit ? .unanswered : nil
}

/// CallKit's configuration: one call at a time, never in the phone's Recents, with our mark on the
/// button that opens the app (`icon`, a template PNG). Set again right before every call is
/// reported or started (Apple DTS, iOS 18.4.1 and later): otherwise the system may never activate
/// the audio session, and the voice is lost. The name CallKit shows is the app's display name
/// (`CFBundleDisplayName`, "FlickerTalk"): `localizedName` is no longer supported since iOS 14.
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

/// The mark CallKit shows on the button of its screen that opens the app: an image set of the
/// app's asset catalog (`gen/apple/Assets.xcassets`).
let callKitIconName = "CallKitIcon"

/// The mark as CallKit takes it, read once from the app's assets; `nil` without them (tests).
let callKitIcon: Data? = UIImage(named: callKitIconName)?.pngData()

/// What CallKit's end action tells the core: a decline for our incoming call whose voice never
/// connected (it may not have its offer yet), an end otherwise.
func endEvent(ours: Bool, outgoing: Bool, connected: Bool) -> CallEvent {
    ours && !outgoing && !connected ? .decline : .end
}

/// Why CallKit's call ended, as it shows in the system.
func endReason(live: Bool) -> CXCallEndedReason { live ? .remoteEnded : .unanswered }

/// The audio session of a call: voice chat, with Bluetooth headsets.
func callAudioOptions() -> AVAudioSession.CategoryOptions { [.allowBluetoothHFP, .allowBluetoothA2DP] }

/// Set before CallKit activates the session (answer and start actions), never activated here: the
/// system activates it and says so in `didActivate`.
func configureCallAudio() {
    try? AVAudioSession.sharedInstance().setCategory(.playAndRecord, mode: .voiceChat, options: callAudioOptions())
}

/// The channel Rust made (`listen_calls`) and the events that wait for it. Its own serial queue:
/// CallKit's delegate (on main) never runs Rust's handler itself, so the handler may call the
/// plugin back (its commands hop to main) without a deadlock.
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

/// Arguments of `registerCallEvents`: the channel, as `__CHANNEL__:<id>`.
struct CallEventsArgs: Decodable {
    let channel: Channel
}

/// Arguments of `setSpeaker`.
struct SpeakerArgs: Decodable {
    let on: Bool
}

/// Arguments of `callStartedOutgoing`.
struct OutgoingArgs: Decodable {
    let name: String
    let video: Bool
}

/// PushKit and CallKit (2026-09-28). Apple wants every VoIP push reported to CallKit at once, so
/// it rings as "FlickerTalk" while the app, woken, connects and reads the offer; then the name
/// comes. Native calls: CallKit owns the call and its audio session in both directions; answer,
/// hang-up, mute and the session's activation go to Rust through `CallEvents`; the WebView only
/// shows what the core does with them (2026-09-29). Used on the main queue.
final class Calls: NSObject, PKPushRegistryDelegate, CXProviderDelegate {
    static let shared = Calls()
    private let registry = PKPushRegistry(queue: .main)
    private let provider: CXProvider
    private let controller = CXCallController(queue: .main)
    let voip = VoipToken()
    private var current: UUID?
    private var answered = false
    /// This phone started the call.
    private var outgoing = false
    /// Answered, or connected: how it ended shows as ended, not unanswered.
    private var live = false
    /// Grows with each CallKit call; audio events carry it (see `CallEvent`).
    private var generation: UInt64 = 0
    /// The generation whose audio session the system activated last: a late `didDeactivate`
    /// belongs to it, not to a call that began since.
    private var sessionGeneration: UInt64 = 0
    /// Whether CallKit activated the session again since the last interruption ended.
    private var reactivated = false
    /// The user's choice for this call: speaker, or the receiver.
    private var speaker = false
    /// When the incoming call was reported, and when it was answered: for `callOverdue`.
    private var ringingSince: Date?
    private var answeredAt: Date?
    /// The core said the call's voice is connected.
    private var voiceConnected = false
    /// Whether incoming calls are reported as video calls, so that answering opens the app.
    private let openAppOnAnswer = OpenAppOnAnswerSetting()

    override init() {
        provider = CXProvider(configuration: callProviderConfiguration(icon: callKitIcon))
        super.init()
        provider.setDelegate(self, queue: .main)
        registry.delegate = self
        registry.desiredPushTypes = [.voIP]
        NotificationCenter.default.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] note in
            self?.interrupted(note)
        }
    }

    /// An interruption (Siri, an alarm…) stops the audio unit. If it ends with "should resume" and
    /// CallKit does not activate the session again within a second, the voice starts again here
    /// (Signal's CallAudioService does the same).
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
            CallEvents.shared.emit(.audioDeactivated(self.sessionGeneration))
            CallEvents.shared.emit(.audioActivated(self.sessionGeneration))
        }
    }

    /// Speaker or receiver, as the user chose (2026-09-28). Applied now and again each time the
    /// system activates the session: an override lasts only while the session is active.
    func setSpeaker(_ on: Bool) {
        speaker = on
        applySpeaker()
    }

    private func applySpeaker() {
        try? AVAudioSession.sharedInstance().overrideOutputAudioPort(outputOverride(speaker: speaker))
    }

    /// The app's own answer button (2026-09-28): CallKit is asked to answer, so the audio session
    /// comes as on the lock screen. `false` when there is no CallKit call to answer: the core
    /// answers by itself.
    func requestAnswer() -> Bool {
        guard let call = current, answerThroughCallKit(callKitCall: true, answered: answered, outgoing: outgoing) else {
            return false
        }
        controller.request(CXTransaction(action: CXAnswerCallAction(call: call))) { error in
            guard error != nil else { return }
            // CallKit refused: the core answers all the same, as before.
            CallEvents.shared.emit(.answer)
        }
        return true
    }

    /// Before every call is reported or started (see `callProviderConfiguration`).
    private func configureProvider() {
        provider.configuration = callProviderConfiguration(icon: callKitIcon)
    }

    /// Ends the call if, `after` seconds from now, it still went nowhere (`callOverdue`).
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

    /// The call's name and video for CallKit: `callUpdate`, with this call's direction and the
    /// switch that opens the app on answer.
    private func update(caller: String, video: Bool) -> CXCallUpdate {
        callUpdate(caller: caller, video: video, outgoing: outgoing, openAppOnAnswer: openAppOnAnswer.on)
    }

    /// Opening the app when an incoming call is answered (experiment, `OpenAppOnAnswer.swift`):
    /// from the next call on; the call ringing now keeps what CallKit was told.
    func setOpenAppOnAnswer(_ on: Bool) {
        openAppOnAnswer.on = on
    }

    /// A new call: what the last one left behind is forgotten.
    private func begin(_ uuid: UUID, outgoing: Bool) {
        current = uuid
        answered = false
        live = false
        voiceConnected = false
        ringingSince = nil
        answeredAt = nil
        speaker = false
        generation += 1
        self.outgoing = outgoing
        // What the user did for an earlier call must not answer or hang up this one.
        CallEvents.shared.forget()
    }

    private func finish() {
        current = nil
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

    /// The core says a call rings: CallKit, also with the app on the screen.
    func ring(caller: String, video: Bool) {
        switch ringWith(callKitCall: current != nil) {
        case .report:
            report(caller: caller, video: video)
        case .update:
            if let current { provider.reportCall(with: current, updated: update(caller: caller, video: video)) }
        }
    }

    /// The core is answering the call (2026-09-29): answered in CallKit, maybe before its offer came
    /// (PushKit reported it as "FlickerTalk"). CallKit only learns who it is; it never rings again.
    func answering(caller: String, video: Bool) {
        guard let current else { return }
        provider.reportCall(with: current, updated: answeringUpdate(caller: caller, video: video, openAppOnAnswer: openAppOnAnswer.on))
    }

    /// The ringing is over. Declined or given up: CallKit lets go. Answered: the call goes on,
    /// with its audio session, until `ended`.
    func stop() {
        guard let call = current, stopEndsCall(answered: answered, outgoing: outgoing) else { return }
        finish()
        provider.reportCall(with: call, endedAt: nil, reason: .remoteEnded)
    }

    /// This phone calls (native calls): CallKit is asked to start it, and its start action sets
    /// the audio session up. `done` hears CallKit's refusal, if any (a phone call going on…).
    func startOutgoing(name: String, video: Bool, done: @escaping (Error?) -> Void) {
        if let old = current { provider.reportCall(with: old, endedAt: nil, reason: .remoteEnded) }
        let uuid = UUID()
        begin(uuid, outgoing: true)
        configureProvider()
        let shown = name.isEmpty ? "FlickerTalk" : name
        let action = CXStartCallAction(call: uuid, handle: CXHandle(type: .generic, value: shown))
        action.isVideo = video
        controller.request(CXTransaction(action: action)) { [weak self] error in
            guard let self else { return }
            if error != nil {
                if self.current == uuid { self.finish() }
            } else if self.current == uuid {
                self.provider.reportCall(with: uuid, updated: self.update(caller: shown, video: video))
            }
            done(error)
        }
    }

    /// The call is connected: an outgoing call shows so; an incoming one was answered in CallKit
    /// (in the app too, through `requestAnswer`) and is live already.
    func connected() {
        voiceConnected = true
        switch connectWith(outgoing: outgoing) {
        case .reportConnected:
            live = true
            if let current { provider.reportOutgoingCall(with: current, connectedAt: nil) }
        case .nothing:
            break
        }
    }

    /// Whether the call has video now (native video, 2026-09-29): CallKit's `hasVideo`, with
    /// either camera on, so that unlocking the phone opens the app on the call.
    func setVideo(_ on: Bool) {
        guard let current else { return }
        provider.reportCall(with: current, updated: videoUpdate(on, outgoing: outgoing, openAppOnAnswer: openAppOnAnswer.on))
    }

    /// The core ended the call, whoever hung up: CallKit lets go, whatever its state.
    func ended() {
        guard let call = current else {
            finish()
            return
        }
        let reason = endReason(live: live || answered)
        finish()
        provider.reportCall(with: call, endedAt: nil, reason: reason)
    }

    // PushKit

    func pushRegistry(_ registry: PKPushRegistry, didUpdate credentials: PKPushCredentials, for type: PKPushType) {
        voip.update(credentials.token)
    }

    func pushRegistry(_ registry: PKPushRegistry, didInvalidatePushTokenFor type: PKPushType) {
        voip.update(nil)
    }

    func pushRegistry(_ registry: PKPushRegistry, didReceiveIncomingPushWith payload: PKPushPayload, for type: PKPushType, completion: @escaping () -> Void) {
        // Every VoIP push rings, or iOS stops delivering them; ours are only ever calls. The core
        // reconnects to the router at once (after `report`, whose new call forgets older events):
        // its socket died with the suspended app, and the offer and the caller's end come by it.
        if current != nil {
            CallEvents.shared.emit(.incoming)
            completion()
            return
        }
        report(caller: "", video: false, done: completion)
        CallEvents.shared.emit(.incoming)
    }

    // CallKit

    func providerDidReset(_ provider: CXProvider) {
        if current != nil { CallEvents.shared.emit(.end) }
        finish()
    }

    func provider(_ provider: CXProvider, perform action: CXStartCallAction) {
        configureCallAudio()
        provider.reportOutgoingCall(with: action.callUUID, startedConnectingAt: nil)
        action.fulfill()
    }

    // Fulfilled at once, not once the voice connects (Apple: that loses the audio, forums thread
    // 747627). The core reconnects and answers when the offer is there; if the voice never
    // connects, `callOverdue` ends the call as failed.
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

    // The system activates the session CallKit's actions set up: Rust starts its audio unit now,
    // and stops it when the session is taken away.
    func provider(_ provider: CXProvider, didActivate audioSession: AVAudioSession) {
        sessionGeneration = generation
        reactivated = true
        applySpeaker()
        CallEvents.shared.emit(.audioActivated(sessionGeneration))
    }

    func provider(_ provider: CXProvider, didDeactivate audioSession: AVAudioSession) {
        CallEvents.shared.emit(.audioDeactivated(sessionGeneration))
    }
}

/// Whether a notification is our router's wake-up (`t: wake`), which says nothing.
func isWakePush(_ userInfo: [AnyHashable: Any]) -> Bool {
    (userInfo["t"] as? String) == "wake"
}

// Registered as early as the plugin loads: a VoIP push may be what launched the app.
private let callsAtLaunch = Calls.shared

/// Apple hands the device token to the app delegate, which Tauri owns: the two answers are added
/// to its class once, and whoever asked for the token hears it here.
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
        // PushKit's token often comes after Apple's: without it a call only shows a notification.
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

/// PushKit's token, and whoever waits a moment for it (2026-09-28). Used on the main queue, like
/// PushKit's delegate.
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

    /// The token now if there is one; else when it comes, or nothing once the time is up.
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

/// Arguments of `callAnswering`, the same as Kotlin's.
struct AnsweringArgs: Decodable {
    let caller: String
    let video: Bool
}

/// Arguments of `startRinging`, the same as Kotlin's.
struct RingingArgs: Decodable {
    let caller: String
    let video: Bool
    let muted: Bool
}

/// Arguments of `openFile`, the same as Kotlin's: a path inside the app and its kind.
struct OpenFileArgs: Decodable {
    let path: String
    let mime: String
}

/// The one item Quick Look shows: the file, as it is on this phone (document viewer, 2026-09-27).
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

/// The file a tap wants shown, if it is there and Quick Look can show it; nil otherwise. Pure,
/// so a test can check it without a screen.
func previewable(path: String) -> PreviewItem? {
    guard !path.isEmpty, FileManager.default.fileExists(atPath: path) else { return nil }
    let item = PreviewItem(url: URL(fileURLWithPath: path))
    return QLPreviewController.canPreview(item) ? item : nil
}

class PlatformPlugin: Plugin {
    /// A login sheet waiting for the provider to send the user back (drive, 2026-09-27).
    private var authSession: ASWebAuthenticationSession?
    private let authAnchor = AuthAnchor()
    /// The WebView the call's video goes under (native video).
    private weak var webView: WKWebView?

    override init() {
        super.init()
        UNUserNotificationCenter.current().delegate = ReminderTaps.shared
        _ = callsAtLaunch
        let install = {
            AppVisibility.install()
            CallIntentActivities.install()
        }
        if Thread.isMainThread { install() } else { DispatchQueue.main.async(execute: install) }
    }

    override func load(webview: WKWebView) {
        webView = webview
    }

    /// An incoming call rings: CallKit when the app is not on the screen (2026-09-28).
    @objc public func startRinging(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(RingingArgs.self)
        DispatchQueue.main.async {
            Calls.shared.ring(caller: args.caller, video: args.video)
            invoke.resolve()
        }
    }

    @objc public func stopRinging(_ invoke: Invoke) throws {
        DispatchQueue.main.async {
            Calls.shared.stop()
            invoke.resolve()
        }
    }

    /// The core listens to native calls (`listen_calls`): events that waited go out now.
    @objc public func registerCallEvents(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(CallEventsArgs.self)
        CallEvents.shared.register(args.channel)
        // Whether the app is on the screen now: the core holds our camera while it is not.
        DispatchQueue.main.async { AppVisibility.tellNow() }
        invoke.resolve()
    }

    // ---- Native video (2026-09-29, docs/video-nativo.md) ----

    /// The call's layers under the WebView, which turns transparent; hidden until `videoLayout`.
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

    /// Where the WebView leaves room for each picture, and whether ours is mirrored.
    @objc public func videoLayout(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(VideoLayoutArgs.self)
        DispatchQueue.main.async {
            CallVideoViews.shared.layout(args)
            invoke.resolve()
        }
    }

    /// The turn of the other side's picture: its layer is laid out by it.
    @objc public func videoShape(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(VideoShapeArgs.self)
        DispatchQueue.main.async {
            CallVideoViews.shared.shape(rotation: args.rotation)
            invoke.resolve()
        }
    }

    /// Takes the layers away and gives the WebView its look back; resolves only once they are out
    /// of the view hierarchy, so Rust may then drop the video devices that own them.
    @objc public func detachVideo(_ invoke: Invoke) throws {
        DispatchQueue.main.async {
            CallVideoViews.shared.detach()
            invoke.resolve()
        }
    }

    /// Opening the app when an incoming call is answered (experiment, 2026-09-29): the core says
    /// whether incoming calls are reported to CallKit as video calls (`OpenAppOnAnswer.swift`).
    @objc public func setOpenAppOnAnswer(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(OpenAppOnAnswerArgs.self)
        DispatchQueue.main.async {
            Calls.shared.setOpenAppOnAnswer(args.on)
            invoke.resolve()
        }
    }

    /// Whether the call has video now: CallKit's `hasVideo`.
    @objc public func callVideo(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(CallVideoArgs.self)
        DispatchQueue.main.async {
            Calls.shared.setVideo(args.on)
            invoke.resolve()
        }
    }

    /// The camera before our camera turns on (§30): asks only if it was never asked; `granted`
    /// says whether it may be used.
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

    /// This phone calls: CallKit takes the call and its audio session (native calls, 2026-09-28).
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

    /// The app's answer button (2026-09-28): CallKit answers, if it has the call; `answered` says.
    @objc public func answerCall(_ invoke: Invoke) throws {
        DispatchQueue.main.async {
            invoke.resolve(["answered": Calls.shared.requestAnswer()])
        }
    }

    /// Speaker or receiver for the call (2026-09-28).
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

    /// Asks for the microphone before a native call, if it was never asked: `granted` says.
    @objc public func requestMicrophone(_ invoke: Invoke) throws {
        if #available(iOS 17.0, *) {
            AVAudioApplication.requestRecordPermission { invoke.resolve(["granted": $0]) }
        } else {
            AVAudioSession.sharedInstance().requestRecordPermission { invoke.resolve(["granted": $0]) }
        }
    }

    /// The core is answering the incoming call (2026-09-29): CallKit gets its name, and no ring.
    @objc public func callAnswering(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(AnsweringArgs.self)
        DispatchQueue.main.async {
            Calls.shared.answering(caller: args.caller, video: args.video)
            invoke.resolve()
        }
    }

    /// Asks to show notifications: the router's wake-ups on iOS are visible ones (2026-09-28).
    @objc public func requestNotifications(_ invoke: Invoke) throws {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in
            invoke.resolve()
        }
    }

    /// Apple's push token, as the router takes it: `gateway:bundle:hex` (2026-09-28).
    @objc public func pushToken(_ invoke: Invoke) throws {
        DispatchQueue.main.async { RemoteToken.shared.ask(invoke) }
    }

    /// The reminder the user tapped to open the app, once (2026-09-27).
    @objc public func pendingReminder(_ invoke: Invoke) throws {
        invoke.resolve(["reminder": ReminderTaps.shared.pending])
        ReminderTaps.shared.pending = ""
    }

    /// Every reminder there is, from the core: what iOS had is replaced. The notification says
    /// only "you have a reminder" unless the core sent a text.
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

    /// A login in the system's sheet (drive, 2026-09-27): `ASWebAuthenticationSession` shows
    /// the provider's page and hands back the URL with our scheme. The WebView sees none of it.
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

    /// Shows a file of the app inside the app, with the system's Quick Look (document viewer,
    /// 2026-09-27): PDF, Office, Pages, text and pictures. Its share button hands the file to
    /// another app, as Android's `openFile` does directly.
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
            // The data source is held only weakly by the controller: keep it as long as it shows.
            objc_setAssociatedObject(preview, "ft.preview.item", item, .OBJC_ASSOCIATION_RETAIN)
            screen.present(preview, animated: true)
            invoke.resolve()
        }
    }

    /// Keeps the storage key in the Keychain; Rust keeps only the marker.
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

    /// The system share sheet (WhatsApp, Signal, mail…) with a text, such as the card link (§32).
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
            // On an iPad the sheet is a popover: it needs an anchor.
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

    // What these commands reject with are keys, not sentences: the app turns them into the
    // user's own language (`plan.trouble.*`), so nothing raw ever reaches the screen.
    /// What the Store already knows about this phone, without asking anyone to buy anything.
    private func entitlements() async -> [StoreEntitlement] {
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

    /// What the Store knows already (§45): the app asks every time it opens, and nothing else.
    @objc public func subscription(_ invoke: Invoke) throws {
        Task {
            invoke.resolve(["until": activeUntil(await entitlements(), now: Date())])
        }
    }

    /// What a year costs, as StoreKit formats it (2026-09-29). Answers `{}` when the Store cannot
    /// say (offline, the product missing): the screen then names no amount at all.
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

    /// The yearly subscription (§45, §47). Apple holds the money and the card; FlickerTalk only
    /// learns until when this phone is paid up.
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
                    // Ask to Buy and the like: not paid for, so it is not pretended to be (§84).
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

/// Where the login sheet is shown: the key window.
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
