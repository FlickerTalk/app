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
