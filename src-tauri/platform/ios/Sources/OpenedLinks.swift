import Foundation

// Links that open the app (Universal Links, 2026-10-08): `https://flickertalk.com/add#…` and
// `/move#…`, verified for this app by the domain's `apple-app-site-association` and the
// `applinks:flickertalk.com` entitlement. iOS hands the link over as a web browsing user activity:
// when the scene connects (a cold start; tao does not read those) or continues (the app ran).
// `CallIntentActivities` hears both, before tao and Tauri. The link waits here until the app asks
// for it (`openedLink`), once; a running app also hears that one came (`LinkOpened`). Nothing acts
// on it here: Rust says what it is, and the page waits for the user's tap.

/// The link a user activity opens the app with: a web browsing one, with an `https` address.
func openedLinkOf(activityType: String, url: URL?) -> String? {
    guard activityType == NSUserActivityTypeBrowsingWeb, let url, url.scheme == "https" else { return nil }
    return url.absoluteString
}

/// The latest link that opened the app, until the app asks for it, once.
final class OpenedLinks {
    static let shared = OpenedLinks()

    private let lock = NSLock()
    private var url = ""
    private var listener: (() -> Void)?

    /// A link came: kept, and a running app hears of it (it then asks for it).
    func put(_ url: String) {
        lock.lock()
        self.url = url
        let told = listener
        lock.unlock()
        told?()
    }

    func take() -> String {
        lock.lock()
        defer { lock.unlock() }
        let taken = url
        url = ""
        return taken
    }

    /// The app listens (`listen_links`): a second listener replaces the first.
    func register(_ listener: @escaping () -> Void) {
        lock.lock()
        self.listener = listener
        lock.unlock()
    }
}
