import AVFAudio
import CallKit
import UIKit
import QuickLook
import XCTest
@testable import tauri_plugin_ft_platform

final class PlatformPluginTests: XCTestCase {
    // What Rust keeps in `storage.key.sealed` on iOS only names the Keychain item: the key itself
    // never leaves the Keychain.
    func testTheSealedFormHoldsNoKey() throws {
        let key = Data(repeating: 7, count: 32)
        let marker = keychainMarker()
        XCTAssertFalse(marker.range(of: key) != nil)
        XCTAssertEqual(String(data: marker, encoding: .utf8), "keychain:v1")
    }

    // The item is readable only on this device, after the first unlock (no iCloud, no backups).
    func testTheKeyStaysOnThisDevice() throws {
        XCTAssertEqual(keyAccessibility(), kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly as String)
    }

    // The share sheet gets the text without stray spaces, and never empty.
    func testOnlyRealTextIsShared() throws {
        XCTAssertEqual(shareableText("  Add me: https://flickertalk.com/add#card\n"), "Add me: https://flickertalk.com/add#card")
        XCTAssertNil(shareableText("   "))
    }

    // What the Store handed back about one entitlement, as the plugin reads it.
    private func entitlement(
        product: String = yearly,
        expires: Date? = Date(timeIntervalSince1970: 1_821_744_000),
        revoked: Date? = nil
    ) -> StoreEntitlement {
        StoreEntitlement(product: product, expires: expires, revoked: revoked)
    }

    // Unlike Play, StoreKit says until when the subscription runs, so the phone keeps that date
    // instead of guessing a year; the core counts time in milliseconds (§45).
    func testTheStoreOwnDateIsKept() throws {
        let runsOut = Date(timeIntervalSince1970: 1_821_744_000)
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        XCTAssertEqual(activeUntil([entitlement(expires: runsOut)], now: now), 1_821_744_000_000)
    }

    func testNothingBoughtIsNoSubscription() throws {
        XCTAssertEqual(activeUntil([], now: Date(timeIntervalSince1970: 1_790_208_000)), 0)
    }

    // Whatever else the Apple ID carries, only our own product pays for FlickerTalk.
    func testOnlyTheYearlyProductCounts() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        XCTAssertEqual(activeUntil([entitlement(product: "com.someone.else.pro")], now: now), 0)
    }

    // Apple gives money back, and says so: a refunded purchase stops paying at once (§84).
    func testARefundedPurchaseStopsCounting() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        let refunded = entitlement(revoked: Date(timeIntervalSince1970: 1_790_100_000))
        XCTAssertEqual(activeUntil([refunded], now: now), 0)
    }

    // A subscription that ran out is not one, however it reached the phone.
    func testAnEntitlementThatRanOutIsNotASubscription() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        let old = entitlement(expires: Date(timeIntervalSince1970: 1_790_100_000))
        XCTAssertEqual(activeUntil([old], now: now), 0)
    }

    // A renewal arrives as a later date: the phone follows the furthest one.
    func testTheFurthestEntitlementCounts() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        let soon = entitlement(expires: Date(timeIntervalSince1970: 1_800_000_000))
        let later = entitlement(expires: Date(timeIntervalSince1970: 1_821_744_000))
        XCTAssertEqual(activeUntil([soon, later], now: now), 1_821_744_000_000)
    }

    // Local reminders (2026-09-27): read leniently, one identifier per reminder, and never more
    // than the 64 iOS allows, the soonest first.
    func testRemindersAreReadAndCappedAtSixtyFour() throws {
        let entries = parseReminders(#"[{"plugin":"com.example.notes","id":"r1","at":5000,"text":"milk"},{"plugin":"","id":"x","at":1}]"#)
        XCTAssertEqual(entries.count, 1)
        XCTAssertEqual(reminderIdentifier(entries[0]), "ft.reminder|com.example.notes|r1")
        XCTAssertEqual(reminderKey("ft.reminder|com.example.notes|r1"), "com.example.notes\nr1")
        XCTAssertNil(reminderKey("something else"))
        XCTAssertEqual(parseReminders("junk").count, 0)
        var many: [ReminderEntry] = []
        for n in 0..<100 { many.append(ReminderEntry(plugin: "p", id: "r\(n)", at: Int64(1000 + (100 - n)), text: nil)) }
        many.append(ReminderEntry(plugin: "p", id: "past", at: 10, text: nil))
        let scheduled = remindersToSchedule(many, now: 500)
        XCTAssertEqual(scheduled.count, 64)
        XCTAssertEqual(scheduled.first?.id, "r99", "the soonest first")
        XCTAssertFalse(scheduled.contains { $0.id == "past" })
    }

    // A login's redirect is only the one with our scheme.
    func testOnlyOurSchemeEndsALogin() throws {
        XCTAssertTrue(isAuthRedirect(URL(string: "com.flickertalk.app:/oauth?code=abc"), scheme: "com.flickertalk.app"))
        XCTAssertFalse(isAuthRedirect(URL(string: "https://evil.example/?code=abc"), scheme: "com.flickertalk.app"))
        XCTAssertFalse(isAuthRedirect(nil, scheme: "com.flickertalk.app"))
    }

    // Document viewer (2026-09-27): a tap shows the file with Quick Look, if it is there.
    func testOnlyAFileThatIsThereCanBePreviewed() throws {
        XCTAssertNil(previewable(path: ""))
        XCTAssertNil(previewable(path: "/nowhere/missing.pdf"))
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("ft-preview-\(UUID().uuidString).txt")
        try "hello".write(to: file, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: file) }
        let item = try XCTUnwrap(previewable(path: file.path))
        XCTAssertEqual(item.previewItemURL, file)
        XCTAssertEqual(item.previewItemTitle, file.lastPathComponent)
        XCTAssertEqual(item.numberOfPreviewItems(in: QLPreviewController()), 1)
    }
    // APNs (2026-09-28): the token names its gateway and the app, so the router pushes to the
    // right place. A development profile says so; an App Store build carries no profile.
    func testTheGatewayComesFromTheProvisioningProfile() {
        let development = "junk<plist><dict><key>Entitlements</key><dict><key>aps-environment</key>\n\t\t<string>development</string></dict></dict></plist>junk"
        let production = development.replacingOccurrences(of: ">development<", with: ">production<")
        XCTAssertEqual(apnsGateway(provisioning: development), "sandbox")
        XCTAssertEqual(apnsGateway(provisioning: production), "production")
        XCTAssertEqual(apnsGateway(provisioning: nil), "production", "no profile: the App Store")
        XCTAssertEqual(apnsGateway(provisioning: "<plist></plist>"), "production")
    }

    func testThePushTargetIsGatewayBundleAndTokenInHex() {
        let token = Data([0x00, 0x0f, 0xab, 0xff])
        XCTAssertEqual(pushTarget(gateway: "sandbox", bundle: "com.flickertalk.app.dev", token: token), "sandbox:com.flickertalk.app.dev:000fabff")
    }

    // Our wake-up says nothing and needs showing only when the app is not on the screen.
    func testOnlyOurWakeUpIsRecognised() {
        XCTAssertTrue(isWakePush(["t": "wake", "s": 0]))
        XCTAssertFalse(isWakePush(["aps": ["alert": "x"]]))
        XCTAssertFalse(isWakePush([:]))
    }
    // Calls (2026-09-28): with PushKit's token too, the router can ring the phone through CallKit.
    func testThePushTargetCarriesPushKitsTokenWhenThereIsOne() {
        let token = Data([0x0a, 0x0b])
        XCTAssertEqual(pushTarget(gateway: "production", bundle: "com.flickertalk.app", token: token, voip: Data([0xff, 0x01])), "production:com.flickertalk.app:0a0b:ff01")
        XCTAssertEqual(pushTarget(gateway: "production", bundle: "com.flickertalk.app", token: token, voip: nil), "production:com.flickertalk.app:0a0b")
    }

    // Every incoming call rings through CallKit, also with the app on the screen (2026-09-28):
    // answered in the app or on the lock screen, it takes the same path (CallKit's answer, then
    // the system activates the audio session), as Signal and react-native-callkeep do.
    func testCallKitRingsEveryIncomingCallOncePerCall() {
        XCTAssertEqual(ringWith(callKitCall: false), .report)
        XCTAssertEqual(ringWith(callKitCall: true), .update, "PushKit already reported it: say who it is")
    }

    // The app's own answer button asks CallKit to answer, so the audio session comes the same way.
    func testTheAppsAnswerGoesThroughCallKit() {
        XCTAssertTrue(answerThroughCallKit(callKitCall: true, answered: false, outgoing: false))
        XCTAssertFalse(answerThroughCallKit(callKitCall: false, answered: false, outgoing: false), "no CallKit call: the core answers")
        XCTAssertFalse(answerThroughCallKit(callKitCall: true, answered: true, outgoing: false), "already answered")
        XCTAssertFalse(answerThroughCallKit(callKitCall: true, answered: false, outgoing: true))
    }

    // Speaker or receiver (2026-09-28): `.playAndRecord` with `.voiceChat` plays on the receiver,
    // which a user holding the phone in front of them barely hears. The user chooses.
    func testTheSpeakerIsAnOverrideOfTheOutput() {
        XCTAssertEqual(outputOverride(speaker: true), .speaker)
        XCTAssertEqual(outputOverride(speaker: false), AVAudioSession.PortOverride.none)
    }

    // An interruption that ends with "should resume" and no new `didActivate` from CallKit: the
    // voice is started again (Signal's CallAudioService does the same).
    func testTheVoiceComesBackAfterAnInterruption() {
        XCTAssertTrue(restartAfterInterruption(shouldResume: true, reactivated: false, callLive: true))
        XCTAssertFalse(restartAfterInterruption(shouldResume: true, reactivated: true, callLive: true), "CallKit gave it back")
        XCTAssertFalse(restartAfterInterruption(shouldResume: false, reactivated: false, callLive: true))
        XCTAssertFalse(restartAfterInterruption(shouldResume: true, reactivated: false, callLive: false))
    }

    // A push that is not ours is not a call, and ours only when it says so.
    func testOnlyOurCallPushIsACall() {
        XCTAssertTrue(isCallPush(["t": "call", "s": 1]))
        XCTAssertFalse(isCallPush(["t": "wake", "s": 1]))
        XCTAssertFalse(isCallPush([:]))
    }

    // Bug of 2026-09-28: iOS hands a tapped notification over off the main thread, and answering
    // it from there made UIKit stop the app ("Call must be made on main thread").
    func testATappedNotificationIsAnsweredOnTheMainThread() {
        let taps = ReminderTaps()
        let answered = expectation(description: "answered")
        DispatchQueue.global().async {
            taps.tapped(identifier: "ft.reminder|com.example.notes|r1") {
                XCTAssertTrue(Thread.isMainThread)
                XCTAssertEqual(taps.pending, "com.example.notes\nr1")
                answered.fulfill()
            }
        }
        wait(for: [answered], timeout: 2)
    }

    func testANotificationOnTheScreenIsAnsweredOnTheMainThread() {
        let taps = ReminderTaps()
        let wake = expectation(description: "wake")
        let other = expectation(description: "other")
        DispatchQueue.global().async {
            taps.presenting(userInfo: ["t": "wake"]) { options in
                XCTAssertTrue(Thread.isMainThread)
                XCTAssertEqual(options, [], "the app on the screen already connects")
                wake.fulfill()
            }
            taps.presenting(userInfo: [:]) { options in
                XCTAssertTrue(Thread.isMainThread)
                XCTAssertEqual(options, [.banner, .sound, .list])
                other.fulfill()
            }
        }
        wait(for: [wake, other], timeout: 2)
    }

    // Bug of 2026-09-28: PushKit's token often comes after Apple's, and the router got only the
    // latter: a call then showed a notification instead of ringing.
    func testTheVoipTokenAlreadyThereIsGivenAtOnce() {
        let voip = VoipToken()
        voip.update(Data([0x01]))
        var given: Data?? = .none
        voip.when(within: 5) { given = .some($0) }
        XCTAssertEqual(given, .some(Data([0x01])))
    }

    func testAVoipTokenThatComesSoonIsWaitedFor() {
        let voip = VoipToken()
        let given = expectation(description: "given")
        voip.when(within: 5) { token in
            XCTAssertEqual(token, Data([0x02]))
            given.fulfill()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { voip.update(Data([0x02])) }
        wait(for: [given], timeout: 2)
    }

    func testWithoutAVoipTokenTheWaitEndsOnceWithNothing() {
        let voip = VoipToken()
        let given = expectation(description: "given")
        given.assertForOverFulfill = true
        voip.when(within: 0.2) { token in
            XCTAssertNil(token)
            given.fulfill()
        }
        wait(for: [given], timeout: 2)
        voip.update(Data([0x03]))
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
    }

    // Native calls (2026-09-28): the events travel as Rust's `call_event` reads them.
    private func json(_ event: CallEvent) -> NSDictionary {
        NSDictionary(dictionary: callEventPayload(event).mapValues { $0 ?? NSNull() })
    }

    func testCallEventsTravelAsTheCoreReadsThem() {
        XCTAssertEqual(json(.answer), ["event": "answer"])
        XCTAssertEqual(json(.end), ["event": "end"])
        XCTAssertEqual(json(.mute(true)), ["event": "mute", "muted": true])
        XCTAssertEqual(json(.mute(false)), ["event": "mute", "muted": false])
        // With the CallKit call's generation: a late event of an older call is told apart.
        XCTAssertEqual(json(.audioActivated(3)), ["event": "audioActivated", "generation": 3])
        XCTAssertEqual(json(.audioDeactivated(2)), ["event": "audioDeactivated", "generation": 2])
        XCTAssertTrue(JSONSerialization.isValidJSONObject(callEventPayload(.mute(true)).mapValues { $0 ?? NSNull() }))
    }

    // PushKit may launch the app, and the user answer on the lock screen, before Rust listens:
    // nothing is lost, and it all arrives in order once it does.
    func testCallEventsWaitUntilTheCoreListens() {
        let queue = CallEventQueue()
        var heard: [CallEvent] = []
        queue.emit(.answer)
        queue.emit(.audioActivated(1))
        XCTAssertEqual(heard, [])
        queue.register { heard.append($0) }
        XCTAssertEqual(heard, [.answer, .audioActivated(1)])
        queue.emit(.mute(true))
        XCTAssertEqual(heard, [.answer, .audioActivated(1), .mute(true)])
    }

    // A new listener (the core started again) replaces the old one and gets no repeats.
    func testANewListenerReplacesTheOld() {
        let queue = CallEventQueue()
        var first: [CallEvent] = []
        var second: [CallEvent] = []
        queue.emit(.answer)
        queue.register { first.append($0) }
        queue.register { second.append($0) }
        queue.emit(.end)
        XCTAssertEqual(first, [.answer])
        XCTAssertEqual(second, [.end])
    }

    // Waiting is bounded, and a new call forgets what an old one left unheard.
    func testWaitingEventsAreBoundedAndForgottenByANewCall() {
        let queue = CallEventQueue(limit: 2)
        var heard: [CallEvent] = []
        queue.emit(.answer)
        queue.emit(.mute(true))
        queue.emit(.end)
        queue.register { heard.append($0) }
        XCTAssertEqual(heard, [.mute(true), .end], "the latest ones are kept")
        let other = CallEventQueue()
        var none: [CallEvent] = []
        other.emit(.end)
        other.forget()
        other.register { none.append($0) }
        XCTAssertEqual(none, [])
    }

    // Bug the native calls would bring: the core stops the ringing once answered, and that used to
    // end the CallKit call, and with it the audio session.
    func testAnAnsweredCallOutlivesTheRinging() {
        XCTAssertTrue(stopEndsCall(answered: false), "declined or given up: CallKit lets go")
        XCTAssertFalse(stopEndsCall(answered: true), "answered: it goes on until callEnded")
        XCTAssertFalse(stopEndsCall(answered: false, outgoing: true), "this phone's own call is not a ringing one")
    }

    // An outgoing call shows as connected; an incoming one was answered in CallKit, and is live.
    func testConnectingReportsAnOutgoingCall() {
        XCTAssertEqual(connectWith(outgoing: true), .reportConnected)
        XCTAssertEqual(connectWith(outgoing: false), .nothing)
    }

    // Recents and the lock screen say what happened: a call nobody picked up is unanswered.
    func testAnEndedCallSaysWhy() {
        XCTAssertEqual(endReason(live: true), .remoteEnded)
        XCTAssertEqual(endReason(live: false), .unanswered)
    }

    // Bug of 2026-09-28: a suspended iPhone rang through PushKit, but its socket to the router was
    // dead and nothing told the core: the offer and the caller's end never came. The push tells it
    // now; a new call forgets what an old one left, never the push that announces it.
    func testACallPushTellsTheCoreToReconnect() {
        XCTAssertEqual(json(.incoming), ["event": "incoming"])
        let queue = CallEventQueue()
        var heard: [CallEvent] = []
        queue.emit(.end)
        queue.forget()
        queue.emit(.incoming)
        queue.register { heard.append($0) }
        XCTAssertEqual(heard, [.incoming])
    }

    // CallKit must not ring for ever (2026-09-28): a call nobody answers ends as unanswered, and an
    // answered one whose voice never connects ends as failed. This phone's own call is the core's.
    func testACallKitCallThatGoesNowhereEndsItself() {
        XCTAssertEqual(callRingLimit, 45, "as the caller's limit and the push's life")
        XCTAssertEqual(callConnectLimit, 30)
        XCTAssertNil(callOverdue(outgoing: false, answered: false, connected: false, ringingFor: 44, answeredFor: nil))
        XCTAssertEqual(callOverdue(outgoing: false, answered: false, connected: false, ringingFor: 46, answeredFor: nil), .unanswered)
        XCTAssertNil(callOverdue(outgoing: false, answered: true, connected: false, ringingFor: 90, answeredFor: 29))
        XCTAssertEqual(callOverdue(outgoing: false, answered: true, connected: false, ringingFor: 90, answeredFor: 31), .failed)
        XCTAssertNil(callOverdue(outgoing: false, answered: true, connected: true, ringingFor: 900, answeredFor: 600))
        XCTAssertNil(callOverdue(outgoing: true, answered: false, connected: false, ringingFor: 600, answeredFor: nil))
    }

    // The caller's end reaches the core, which stops the ringing and ends the call: a CallKit call
    // that still rings lets go at the first of the two.
    func testTheCallersEndLetsARingingCallKitCallGo() {
        XCTAssertTrue(stopEndsCall(answered: false, outgoing: false))
        XCTAssertEqual(endReason(live: false), .unanswered)
    }

    // iOS 18.4.1 and later (Apple DTS, developer forums thread 783870): the configuration is set
    // again right before every call is reported or started, or `didActivate` may never come.
    func testTheProviderConfigurationIsOneCallAtATime() {
        let configuration = callProviderConfiguration(icon: nil)
        XCTAssertTrue(configuration.supportsVideo)
        XCTAssertEqual(configuration.maximumCallsPerCallGroup, 1)
        XCTAssertEqual(configuration.maximumCallGroups, 1)
        XCTAssertEqual(configuration.supportedHandleTypes, [.generic])
        XCTAssertFalse(configuration.includesCallsInRecents)
    }

    // Temporary diagnostics (2026-09-28): the audio session is described by state and port types
    // only, never by a device's name.
    func testTheAudioSessionIsDescribedWithoutNames() {
        let summary = audioSessionSummary(category: "AVAudioSessionCategoryPlayAndRecord", mode: "AVAudioSessionModeVoiceChat", outputs: ["Receiver"], inputs: ["MicrophoneBuiltIn"])
        XCTAssertEqual(summary, "category=AVAudioSessionCategoryPlayAndRecord mode=AVAudioSessionModeVoiceChat out=Receiver in=MicrophoneBuiltIn")
    }

    // Bug of 2026-09-29: with the app closed, CallKit (rung by PushKit) was declined before the
    // offer came; the core had nothing to end, and the offer then rang again. CallKit's end of an
    // incoming call whose voice never connected is a decline, which waits for the offer and
    // declines it. The end of a connected call, of this phone's own call or of an older call is
    // an end, as before. (What the user did in CallKit no longer waits for the WebView at all: it
    // goes to the core, and the WebView shows what the core does.)
    func testCallKitsEndOfACallThatNeverConnectedIsADecline() {
        XCTAssertEqual(json(.decline), ["event": "decline"])
        XCTAssertEqual(endEvent(ours: true, outgoing: false, connected: false), .decline)
        XCTAssertEqual(endEvent(ours: true, outgoing: false, connected: true), .end)
        XCTAssertEqual(endEvent(ours: true, outgoing: true, connected: false), .end)
        XCTAssertEqual(endEvent(ours: false, outgoing: false, connected: false), .end, "an older call's end")
    }

    // The button of CallKit's screen that opens the app shows our mark (2026-09-29): a monochrome
    // template, as CallKit wants it.
    func testCallKitShowsOurMark() {
        let icon = Data([0x89, 0x50, 0x4E, 0x47])
        XCTAssertEqual(callProviderConfiguration(icon: icon).iconTemplateImageData, icon)
        XCTAssertNil(callProviderConfiguration(icon: nil).iconTemplateImageData)
    }

    // The mark is in the app's asset catalog (`gen/apple`, which the build keeps): 40 pt at 1x, 2x
    // and 3x, rendered as a template, a white glyph on a transparent background.
    func testTheCallKitMarkIsATemplateInTheAppsAssets() throws {
        let set = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("gen/apple/Assets.xcassets/\(callKitIconName).imageset")
        let contents = try JSONSerialization.jsonObject(with: Data(contentsOf: set.appendingPathComponent("Contents.json"))) as? [String: Any]
        let properties = contents?["properties"] as? [String: Any]
        XCTAssertEqual(properties?["template-rendering-intent"] as? String, "template")
        let images = contents?["images"] as? [[String: Any]] ?? []
        XCTAssertEqual(Set(images.compactMap { $0["scale"] as? String }), ["1x", "2x", "3x"])
        for image in images {
            let scale = Int((image["scale"] as? String ?? "1x").dropLast()) ?? 1
            let file = try XCTUnwrap(image["filename"] as? String)
            let png = try XCTUnwrap(UIImage(data: Data(contentsOf: set.appendingPathComponent(file))))
            XCTAssertEqual(png.size.width * png.scale, CGFloat(40 * scale))
            XCTAssertEqual(png.size.height * png.scale, CGFloat(40 * scale))
            let alpha = try XCTUnwrap(png.cgImage?.alphaInfo)
            XCTAssertTrue([.premultipliedLast, .last, .premultipliedFirst, .first].contains(alpha), "a transparent background")
        }
    }

    // The call's audio: Bluetooth headsets work, both hands-free and high quality output.
    func testCallAudioWorksWithBluetooth() {
        XCTAssertTrue(callAudioOptions().contains(.allowBluetoothHFP))
        XCTAssertTrue(callAudioOptions().contains(.allowBluetoothA2DP))
    }
}
