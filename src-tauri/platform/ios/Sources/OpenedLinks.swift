import Foundation

// Universal Links arrive as a browsing user activity on scene connect (tao ignores it there) or continue.
// Nothing acts on a link here, the page waits for the user's tap.

func openedLinkOf(activityType: String, url: URL?) -> String? {
    guard activityType == NSUserActivityTypeBrowsingWeb, let url, url.scheme == "https" else { return nil }
    return url.absoluteString
}

final class OpenedLinks {
    static let shared = OpenedLinks()

    private let lock = NSLock()
    private var url = ""
    private var listener: (() -> Void)?

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

    func register(_ listener: @escaping () -> Void) {
        lock.lock()
        self.listener = listener
        lock.unlock()
    }
}
