import AuthenticationServices
import Foundation
import Tauri
import UIKit

func isAuthRedirect(_ url: URL?, scheme: String) -> Bool {
    guard let url, !scheme.isEmpty else { return false }
    return url.scheme?.lowercased() == scheme.lowercased()
}

class AuthorizeArgs: Decodable {
    let url: String
    let scheme: String
}

final class AuthAnchor: NSObject, ASWebAuthenticationPresentationContextProviding {
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows)
            .first { $0.isKeyWindow } ?? ASPresentationAnchor()
    }
}

extension PlatformPlugin {
    // the WebView never sees the login
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
}
