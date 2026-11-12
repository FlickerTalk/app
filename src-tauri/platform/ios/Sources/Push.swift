import Foundation
import ObjectiveC
import Tauri
import UIKit
import UserNotifications

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

func isWakePush(_ userInfo: [AnyHashable: Any]) -> Bool {
    (userInfo["t"] as? String) == "wake"
}

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

extension PlatformPlugin {
    @objc public func requestNotifications(_ invoke: Invoke) throws {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in
            invoke.resolve()
        }
    }

    // the router expects gateway:bundle:hex[:voip-hex]
    @objc public func pushToken(_ invoke: Invoke) throws {
        DispatchQueue.main.async { RemoteToken.shared.ask(invoke) }
    }
}
