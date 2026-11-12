import XCTest
@testable import tauri_plugin_ft_platform

final class PushTests: XCTestCase {
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

    func testOnlyOurWakeUpIsRecognised() {
        XCTAssertTrue(isWakePush(["t": "wake", "s": 0]))
        XCTAssertFalse(isWakePush(["aps": ["alert": "x"]]))
        XCTAssertFalse(isWakePush([:]))
    }

    func testThePushTargetCarriesPushKitsTokenWhenThereIsOne() {
        let token = Data([0x0a, 0x0b])
        XCTAssertEqual(pushTarget(gateway: "production", bundle: "com.flickertalk.app", token: token, voip: Data([0xff, 0x01])), "production:com.flickertalk.app:0a0b:ff01")
        XCTAssertEqual(pushTarget(gateway: "production", bundle: "com.flickertalk.app", token: token, voip: nil), "production:com.flickertalk.app:0a0b")
    }

    func testOnlyOurCallPushIsACall() {
        XCTAssertTrue(isCallPush(["t": "call", "s": 1]))
        XCTAssertFalse(isCallPush(["t": "wake", "s": 1]))
        XCTAssertFalse(isCallPush([:]))
    }

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
}
