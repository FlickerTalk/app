import AVFAudio
import XCTest
@testable import tauri_plugin_ft_platform

final class CallAudioRouteTests: XCTestCase {
    func testTheRouteIsTheReceiverTheSpeakerOrAHeadset() {
        XCTAssertEqual(callOutput(of: [.builtInReceiver]), .receiver)
        XCTAssertEqual(callOutput(of: [.builtInSpeaker]), .speaker)
        XCTAssertEqual(callOutput(of: [.headphones]), .headset)
        XCTAssertEqual(callOutput(of: [.bluetoothHFP]), .headset)
        XCTAssertEqual(callOutput(of: [.bluetoothA2DP]), .headset)
        XCTAssertEqual(callOutput(of: [.bluetoothLE]), .headset)
        XCTAssertEqual(callOutput(of: [.usbAudio]), .headset)
        XCTAssertEqual(callOutput(of: [.carAudio]), .headset)
        XCTAssertEqual(callOutput(of: []), .receiver, "no route yet")
    }

    func testTheCallPrefersAHeadsetMicrophone() {
        XCTAssertEqual(headsetInput(inRoute: [.builtInMic], available: [.builtInMic, .bluetoothHFP]), .bluetoothHFP)
        XCTAssertEqual(headsetInput(inRoute: [.builtInMic], available: [.builtInMic, .headsetMic]), .headsetMic)
        XCTAssertEqual(headsetInput(inRoute: [.builtInMic], available: [.builtInMic, .usbAudio]), .usbAudio)
        XCTAssertEqual(headsetInput(inRoute: [.headsetMic], available: [.builtInMic, .headsetMic, .bluetoothHFP]), .headsetMic, "the route's own headset stays")
        XCTAssertEqual(headsetInput(inRoute: [.builtInMic], available: [.builtInMic, .headsetMic, .bluetoothHFP]), .bluetoothHFP)
        XCTAssertNil(headsetInput(inRoute: [.builtInMic], available: [.builtInMic]))
        XCTAssertNil(headsetInput(inRoute: [], available: []))
    }

    func testHeadphonesAreThereByTheirOutputOrTheirMicrophone() {
        XCTAssertTrue(hasHeadset(outputs: [.headphones], available: [.builtInMic]))
        XCTAssertTrue(hasHeadset(outputs: [.builtInSpeaker], available: [.builtInMic, .bluetoothHFP]))
        XCTAssertTrue(hasHeadset(outputs: [.builtInReceiver], available: [.builtInMic, .headsetMic]))
        XCTAssertFalse(hasHeadset(outputs: [.builtInReceiver], available: [.builtInMic]))
        XCTAssertFalse(hasHeadset(outputs: [.builtInSpeaker], available: [.builtInMic]))
    }

    func testTheSpeakerNeverTakesTheCallFromHeadphonesByItself() {
        XCTAssertFalse(speakerOn(choice: nil, prefersSpeaker: false, headset: false), "a voice call starts on the receiver")
        XCTAssertFalse(speakerOn(choice: nil, prefersSpeaker: false, headset: true))
        XCTAssertTrue(speakerOn(choice: nil, prefersSpeaker: true, headset: false), "video: speaker, not the receiver")
        XCTAssertFalse(speakerOn(choice: nil, prefersSpeaker: true, headset: true), "video with headphones: the headphones")
        XCTAssertTrue(speakerOn(choice: true, prefersSpeaker: false, headset: true), "the user's speaker, even with headphones")
        XCTAssertFalse(speakerOn(choice: false, prefersSpeaker: true, headset: false), "the user's receiver, even in a video call")
    }

    func testHeadphonesThatComeInTakeTheCall() {
        XCTAssertNil(choiceAfterRouteChange(true, reason: .newDeviceAvailable))
        XCTAssertNil(choiceAfterRouteChange(false, reason: .newDeviceAvailable))
        XCTAssertEqual(choiceAfterRouteChange(true, reason: .oldDeviceUnavailable), true)
        XCTAssertEqual(choiceAfterRouteChange(false, reason: .override), false)
        XCTAssertNil(choiceAfterRouteChange(nil, reason: .categoryChange))
    }

    private func choice(
        _ choice: Bool?,
        after reason: AVAudioSession.RouteChangeReason,
        from before: [AVAudioSession.Port],
        to after: [AVAudioSession.Port],
        available: [AVAudioSession.Port] = [.builtInMic],
        video: Bool = false
    ) -> Bool? {
        choiceAfterRouteChange(choice, reason: reason, from: before, to: after, available: available, prefersSpeaker: video)
    }

    func testTheSpeakerPickedOnTheSystemCallScreenStaysThroughLaterChanges() {
        let picked = choice(nil, after: .override, from: [.builtInReceiver], to: [.builtInSpeaker])
        XCTAssertEqual(picked, true)
        let later: [AVAudioSession.RouteChangeReason] = [.categoryChange, .wakeFromSleep, .oldDeviceUnavailable, .noSuitableRouteForCategory]
        for reason in later {
            let kept = choice(picked, after: reason, from: [.builtInSpeaker], to: [.builtInReceiver])
            XCTAssertEqual(kept, true, "reason \(reason.rawValue)")
            XCTAssertTrue(speakerOn(choice: kept, prefersSpeaker: false, headset: false), "reason \(reason.rawValue)")
        }
        let headphones: [AVAudioSession.Port] = [.builtInMic, .bluetoothHFP]
        let overHeadphones = choice(nil, after: .override, from: [.bluetoothHFP], to: [.builtInSpeaker], available: headphones)
        XCTAssertEqual(overHeadphones, true)
        XCTAssertEqual(choice(overHeadphones, after: .oldDeviceUnavailable, from: [.builtInSpeaker], to: [.builtInSpeaker]), true, "the headphones went away")
    }

    func testTheEarpiecePickedOnTheSystemCallScreenStaysAfterTheAppHadTheSpeakerOn() {
        let inVideo = choice(nil, after: .override, from: [.builtInSpeaker], to: [.builtInReceiver], video: true)
        XCTAssertEqual(inVideo, false)
        let afterWake = choice(inVideo, after: .wakeFromSleep, from: [.builtInReceiver], to: [.builtInReceiver], video: true)
        XCTAssertEqual(afterWake, false)
        XCTAssertFalse(speakerOn(choice: afterWake, prefersSpeaker: true, headset: false))
        XCTAssertEqual(choice(true, after: .override, from: [.builtInSpeaker], to: [.builtInReceiver]), false, "the app's speaker button had turned it on")
    }

    func testTheSystemCallScreenCanMoveTheCallBetweenHeadphonesAndTheSpeaker() {
        let headphones: [AVAudioSession.Port] = [.builtInMic, .bluetoothHFP]
        let speaker = choice(nil, after: .override, from: [.bluetoothHFP], to: [.builtInSpeaker], available: headphones)
        XCTAssertEqual(speaker, true)
        XCTAssertTrue(speakerOn(choice: speaker, prefersSpeaker: false, headset: true))
        let back = choice(speaker, after: .override, from: [.builtInSpeaker], to: [.bluetoothHFP], available: headphones)
        XCTAssertEqual(back, false)
        XCTAssertFalse(speakerOn(choice: back, prefersSpeaker: false, headset: true))
        XCTAssertEqual(choice(nil, after: .override, from: [.headphones], to: [.builtInSpeaker], video: true), true, "wired headphones without a microphone")
    }

    func testTheAppsOwnRouteIsNotTakenForAChoice() {
        XCTAssertNil(choice(nil, after: .override, from: [.builtInReceiver], to: [.builtInSpeaker], video: true), "video starts on the speaker")
        XCTAssertEqual(choice(true, after: .override, from: [.builtInReceiver], to: [.builtInSpeaker]), true)
        XCTAssertEqual(choice(false, after: .override, from: [.builtInSpeaker], to: [.builtInReceiver]), false)
        XCTAssertNil(choice(nil, after: .override, from: [.builtInReceiver], to: [.bluetoothHFP], available: [.builtInMic, .bluetoothHFP]), "the headset microphone")
        XCTAssertNil(choice(nil, after: .override, from: [.builtInSpeaker], to: [.bluetoothHFP], available: [.builtInMic, .bluetoothHFP], video: true), "video gives the speaker up to headphones")
        XCTAssertNil(choice(nil, after: .override, from: [.builtInSpeaker], to: [.builtInSpeaker]), "nothing moved")
    }

    func testWhatTheSystemDoesByItselfIsNotAChoice() {
        XCTAssertNil(choice(nil, after: .oldDeviceUnavailable, from: [.bluetoothHFP], to: [.builtInReceiver], video: true), "headphones gone in a video call")
        XCTAssertEqual(choice(true, after: .oldDeviceUnavailable, from: [.builtInSpeaker], to: [.builtInReceiver]), true)
        XCTAssertNil(choice(nil, after: .wakeFromSleep, from: [.builtInSpeaker], to: [.builtInReceiver], video: true))
        XCTAssertNil(choice(nil, after: .categoryChange, from: [.builtInSpeaker], to: [.builtInReceiver], video: true))
        XCTAssertNil(choice(nil, after: .routeConfigurationChange, from: [.builtInReceiver], to: [.builtInSpeaker]))
        XCTAssertNil(choice(nil, after: .unknown, from: [.builtInReceiver], to: [.builtInSpeaker]))
    }

    func testHeadphonesStillTakeTheCallAfterAPickAndGiveItBackToTheEarpiece() {
        let picked = choice(nil, after: .override, from: [.builtInReceiver], to: [.builtInSpeaker])
        XCTAssertEqual(picked, true)
        let withHeadphones = choice(picked, after: .newDeviceAvailable, from: [.builtInSpeaker], to: [.bluetoothHFP], available: [.builtInMic, .bluetoothHFP])
        XCTAssertNil(withHeadphones)
        XCTAssertFalse(speakerOn(choice: withHeadphones, prefersSpeaker: false, headset: true))
        let withoutHeadphones = choice(withHeadphones, after: .oldDeviceUnavailable, from: [.bluetoothHFP], to: [.builtInReceiver])
        XCTAssertNil(withoutHeadphones)
        XCTAssertFalse(speakerOn(choice: withoutHeadphones, prefersSpeaker: false, headset: false))
    }

    func testOnlyAnOverrideIsReadAsAChoiceAndItNeverRoutesAgain() {
        for raw: UInt in 0...8 {
            guard let reason = AVAudioSession.RouteChangeReason(rawValue: raw) else { continue }
            let picked = choice(nil, after: reason, from: [.builtInReceiver], to: [.builtInSpeaker]) == true
            XCTAssertEqual(picked, reason == .override, "reason \(raw)")
            if picked { XCTAssertFalse(reroutes(after: reason), "reason \(raw)") }
        }
    }

    func testTheRouteIsDecidedAgainWhenADeviceComesOrGoes() {
        XCTAssertTrue(reroutes(after: .newDeviceAvailable))
        XCTAssertTrue(reroutes(after: .oldDeviceUnavailable))
        XCTAssertTrue(reroutes(after: .categoryChange))
        XCTAssertTrue(reroutes(after: .wakeFromSleep))
        XCTAssertTrue(reroutes(after: .noSuitableRouteForCategory))
        XCTAssertFalse(reroutes(after: .override))
        XCTAssertFalse(reroutes(after: .routeConfigurationChange))
        XCTAssertFalse(reroutes(after: .unknown))
    }

    func testTheSessionIsTheCallsByCategoryAndMode() {
        XCTAssertTrue(isCallSession(category: .playAndRecord, mode: .voiceChat))
        XCTAssertFalse(isCallSession(category: .playAndRecord, mode: .default))
        XCTAssertFalse(isCallSession(category: .playAndRecord, mode: .videoChat))
        XCTAssertFalse(isCallSession(category: .playback, mode: .voiceChat))
        XCTAssertFalse(isCallSession(category: .soloAmbient, mode: .default))
    }

    func testTheRouteTravelsToTheCore() {
        XCTAssertEqual(callEventPayload(.route(.headset)) as NSDictionary, ["event": "route", "route": "headset"] as NSDictionary)
        XCTAssertEqual(callEventPayload(.route(.speaker)) as NSDictionary, ["event": "route", "route": "speaker"] as NSDictionary)
        XCTAssertEqual(callEventPayload(.route(.receiver)) as NSDictionary, ["event": "route", "route": "receiver"] as NSDictionary)
    }

    func testTheSpeakerIsAnOverrideOfTheOutput() {
        XCTAssertEqual(outputOverride(speaker: true), .speaker)
        XCTAssertEqual(outputOverride(speaker: false), AVAudioSession.PortOverride.none)
    }

    func testTheVoiceComesBackAfterAnInterruption() {
        XCTAssertTrue(restartAfterInterruption(shouldResume: true, reactivated: false, callLive: true))
        XCTAssertFalse(restartAfterInterruption(shouldResume: true, reactivated: true, callLive: true), "CallKit gave it back")
        XCTAssertFalse(restartAfterInterruption(shouldResume: false, reactivated: false, callLive: true))
        XCTAssertFalse(restartAfterInterruption(shouldResume: true, reactivated: false, callLive: false))
    }

    func testCallAudioWorksWithBluetooth() {
        XCTAssertTrue(callAudioOptions().contains(.allowBluetoothHFP))
        XCTAssertTrue(callAudioOptions().contains(.allowBluetoothA2DP))
    }
}
