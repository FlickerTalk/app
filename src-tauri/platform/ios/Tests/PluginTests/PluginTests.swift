import AVFAudio
import CallKit
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

    // On the screen the app rings itself; in the background CallKit does, once per call.
    func testCallKitRingsOnlyWhenTheAppIsNotOnTheScreen() {
        XCTAssertEqual(ringWith(appActive: true, callKitCall: false), .app)
        XCTAssertEqual(ringWith(appActive: false, callKitCall: false), .report)
        XCTAssertEqual(ringWith(appActive: false, callKitCall: true), .update, "PushKit already reported it: say who it is")
        XCTAssertEqual(ringWith(appActive: true, callKitCall: true), .update)
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
        XCTAssertEqual(json(.audioActivated), ["event": "audioActivated"])
        XCTAssertEqual(json(.audioDeactivated), ["event": "audioDeactivated"])
        XCTAssertTrue(JSONSerialization.isValidJSONObject(callEventPayload(.mute(true)).mapValues { $0 ?? NSNull() }))
    }

    // PushKit may launch the app, and the user answer on the lock screen, before Rust listens:
    // nothing is lost, and it all arrives in order once it does.
    func testCallEventsWaitUntilTheCoreListens() {
        let queue = CallEventQueue()
        var heard: [CallEvent] = []
        queue.emit(.answer)
        queue.emit(.audioActivated)
        XCTAssertEqual(heard, [])
        queue.register { heard.append($0) }
        XCTAssertEqual(heard, [.answer, .audioActivated])
        queue.emit(.mute(true))
        XCTAssertEqual(heard, [.answer, .audioActivated, .mute(true)])
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

    // An outgoing call shows as connected; a call answered in the app's own screen (it rang
    // there, CallKit never heard of it) joins CallKit, which gives it the audio session.
    func testConnectingReportsOrJoinsCallKit() {
        XCTAssertEqual(connectWith(callKitCall: true, outgoing: true), .reportConnected)
        XCTAssertEqual(connectWith(callKitCall: false, outgoing: false), .join)
        XCTAssertEqual(connectWith(callKitCall: false, outgoing: true), .join)
        XCTAssertEqual(connectWith(callKitCall: true, outgoing: false), .nothing, "answered in CallKit: already live")
    }

    // Recents and the lock screen say what happened: a call nobody picked up is unanswered.
    func testAnEndedCallSaysWhy() {
        XCTAssertEqual(endReason(live: true), .remoteEnded)
        XCTAssertEqual(endReason(live: false), .unanswered)
    }

    // The call's audio: Bluetooth headsets work, both hands-free and high quality output.
    func testCallAudioWorksWithBluetooth() {
        XCTAssertTrue(callAudioOptions().contains(.allowBluetoothHFP))
        XCTAssertTrue(callAudioOptions().contains(.allowBluetoothA2DP))
    }
}
