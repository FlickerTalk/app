import AVFAudio
import CallKit
import Foundation
import PushKit
import Tauri
import UIKit
import UserNotifications

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

struct SpeakerArgs: Decodable {
    let on: Bool
}

struct OutgoingArgs: Decodable {
    let name: String
    let video: Bool
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

extension PlatformPlugin {
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
}
