import AVFAudio
import XCTest
@testable import tauri_plugin_ft_platform

// Where a native call's voice plays (2026-10-28): headphones, wired or Bluetooth, connected before
// the call or during it, take the call; the speaker is the user's choice, or a video call's
// preference over the receiver, never over headphones.
final class CallAudioRouteTests: XCTestCase {
    // The call screen shows the route as one of three: the receiver, the speaker or a headset.
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

    // A headset's microphone carries the call both ways: the one in the route already, else the
    // first one there is, Bluetooth hands-free first; none with only the iPhone's own microphone.
    func testTheCallPrefersAHeadsetMicrophone() {
        XCTAssertEqual(headsetInput(inRoute: [.builtInMic], available: [.builtInMic, .bluetoothHFP]), .bluetoothHFP)
        XCTAssertEqual(headsetInput(inRoute: [.builtInMic], available: [.builtInMic, .headsetMic]), .headsetMic)
        XCTAssertEqual(headsetInput(inRoute: [.builtInMic], available: [.builtInMic, .usbAudio]), .usbAudio)
        XCTAssertEqual(headsetInput(inRoute: [.headsetMic], available: [.builtInMic, .headsetMic, .bluetoothHFP]), .headsetMic, "the route's own headset stays")
        XCTAssertEqual(headsetInput(inRoute: [.builtInMic], available: [.builtInMic, .headsetMic, .bluetoothHFP]), .bluetoothHFP)
        XCTAssertNil(headsetInput(inRoute: [.builtInMic], available: [.builtInMic]))
        XCTAssertNil(headsetInput(inRoute: [], available: []))
    }

    // Headphones are there when the route plays to them (wired headphones have no microphone) or
    // a headset microphone is available (the speaker override hides the headset's output).
    func testHeadphonesAreThereByTheirOutputOrTheirMicrophone() {
        XCTAssertTrue(hasHeadset(outputs: [.headphones], available: [.builtInMic]))
        XCTAssertTrue(hasHeadset(outputs: [.builtInSpeaker], available: [.builtInMic, .bluetoothHFP]))
        XCTAssertTrue(hasHeadset(outputs: [.builtInReceiver], available: [.builtInMic, .headsetMic]))
        XCTAssertFalse(hasHeadset(outputs: [.builtInReceiver], available: [.builtInMic]))
        XCTAssertFalse(hasHeadset(outputs: [.builtInSpeaker], available: [.builtInMic]))
    }

    // The bug of 2026-10-28: the speaker override beats any headset, and a video call turned it on
    // by itself. Now the user's choice decides; with none, a call that had video prefers the
    // speaker to the receiver, never to headphones.
    func testTheSpeakerNeverTakesTheCallFromHeadphonesByItself() {
        XCTAssertFalse(speakerOn(choice: nil, prefersSpeaker: false, headset: false), "a voice call starts on the receiver")
        XCTAssertFalse(speakerOn(choice: nil, prefersSpeaker: false, headset: true))
        XCTAssertTrue(speakerOn(choice: nil, prefersSpeaker: true, headset: false), "video: speaker, not the receiver")
        XCTAssertFalse(speakerOn(choice: nil, prefersSpeaker: true, headset: true), "video with headphones: the headphones")
        XCTAssertTrue(speakerOn(choice: true, prefersSpeaker: false, headset: true), "the user's speaker, even with headphones")
        XCTAssertFalse(speakerOn(choice: false, prefersSpeaker: true, headset: false), "the user's receiver, even in a video call")
    }

    // Headphones that come in during the call take it, as on a phone call: the user's speaker
    // choice is forgotten (the system drops the override itself). Any other change keeps it.
    func testHeadphonesThatComeInTakeTheCall() {
        XCTAssertNil(choiceAfterRouteChange(true, reason: .newDeviceAvailable))
        XCTAssertNil(choiceAfterRouteChange(false, reason: .newDeviceAvailable))
        XCTAssertEqual(choiceAfterRouteChange(true, reason: .oldDeviceUnavailable), true)
        XCTAssertEqual(choiceAfterRouteChange(false, reason: .override), false)
        XCTAssertNil(choiceAfterRouteChange(nil, reason: .categoryChange))
    }

    // The route is decided again when a device comes or goes, or the system moved it by itself;
    // never after our own override (that would answer each change with another).
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

    // Something else in the app (the WebView's media) may change the session during a call: it is
    // set back once per call when its category or mode is no longer the call's. Not by its options,
    // which the system may report otherwise than they were set: that would set it again and again.
    func testTheSessionIsTheCallsByCategoryAndMode() {
        XCTAssertTrue(isCallSession(category: .playAndRecord, mode: .voiceChat))
        XCTAssertFalse(isCallSession(category: .playAndRecord, mode: .default))
        XCTAssertFalse(isCallSession(category: .playAndRecord, mode: .videoChat))
        XCTAssertFalse(isCallSession(category: .playback, mode: .voiceChat))
        XCTAssertFalse(isCallSession(category: .soloAmbient, mode: .default))
    }

    // The call screen hears the route through the core, as `{"event": "route", "route": …}`.
    func testTheRouteTravelsToTheCore() {
        XCTAssertEqual(callEventPayload(.route(.headset)) as NSDictionary, ["event": "route", "route": "headset"] as NSDictionary)
        XCTAssertEqual(callEventPayload(.route(.speaker)) as NSDictionary, ["event": "route", "route": "speaker"] as NSDictionary)
        XCTAssertEqual(callEventPayload(.route(.receiver)) as NSDictionary, ["event": "route", "route": "receiver"] as NSDictionary)
    }
}
