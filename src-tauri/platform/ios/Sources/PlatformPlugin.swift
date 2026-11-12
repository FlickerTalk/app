import AuthenticationServices
import Foundation
import SwiftRs
import Tauri
import UserNotifications
import WebKit

// registered at load, a VoIP push may be what launched the app
private let callsAtLaunch = Calls.shared

class PlatformPlugin: Plugin {
    var authSession: ASWebAuthenticationSession?
    let authAnchor = AuthAnchor()
    private(set) weak var webView: WKWebView?

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
}

@_cdecl("init_plugin_ft_platform")
func initPlugin() -> Plugin {
    return PlatformPlugin()
}
