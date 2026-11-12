import AVFoundation
import CallKit
import Intents
import ObjectiveC
import UIKit
import WebKit

// Rust owns the call's video layers. They sit under the transparent WebView, all on the main thread.

// with user-scalable=no a CSS pixel is a UIKit point from the viewport origin. The scroll view's offset
// is -inset when UIKit keeps the page out of the safe areas, 0 with viewport-fit=cover.
func webViewRect(css: CGRect, zoom: CGFloat, contentOffset: CGPoint) -> CGRect {
    CGRect(
        x: css.origin.x * zoom - contentOffset.x,
        y: css.origin.y * zoom - contentOffset.y,
        width: css.width * zoom,
        height: css.height * zoom
    )
}

// frame is undefined under the transform the engine sets, so use bounds and position (swapped on a quarter turn)
func remoteLayerGeometry(room: CGSize, rotation: Int) -> (bounds: CGRect, position: CGPoint) {
    let quarter = rotation % 180 != 0
    let size = quarter ? CGSize(width: room.height, height: room.width) : room
    return (CGRect(origin: .zero, size: size), CGPoint(x: room.width / 2, y: room.height / 2))
}

// set it every time, a camera switch makes a new connection with AVFoundation's default again
func previewMirrored(mirrorLocal: Bool, supported: Bool) -> Bool? {
    supported ? mirrorLocal : nil
}

enum CameraAccess: Equatable {
    case granted
    case ask
    case denied
}

func cameraAccess(_ status: AVAuthorizationStatus) -> CameraAccess {
    switch status {
    case .authorized: return .granted
    case .notDetermined: return .ask
    case .denied, .restricted: return .denied
    @unknown default: return .denied
    }
}

// inactive (CallKit's screen, notification centre) still counts as visible
func appVisible(_ state: UIApplication.State) -> Bool {
    state != .background
}

// CXCallUpdate only sends what's set, so the name stays
func videoUpdate(_ on: Bool) -> CXCallUpdate {
    let update = CXCallUpdate()
    update.hasVideo = on
    return update
}

// CallKit's Video button sends INStartCallIntent with the video capability, or the older INStartVideoCallIntent
func asksForVideo(activityType: String, capability: INCallCapability?) -> Bool {
    switch activityType {
    case "INStartVideoCallIntent": return true
    case "INStartCallIntent": return capability == .videoCall
    default: return false
    }
}

func asksForVideo(_ activity: NSUserActivity) -> Bool {
    let intent = activity.interaction?.intent as? INStartCallIntent
    return asksForVideo(activityType: activity.activityType, capability: intent?.callCapability)
}

// raw CALayer addresses owned by Rust, 0 = none
struct AttachVideoArgs: Decodable {
    let remoteLayer: UInt64
    let localLayer: UInt64
}

struct VideoRectArgs: Decodable {
    let x: CGFloat
    let y: CGFloat
    let width: CGFloat
    let height: CGFloat

    var rect: CGRect { CGRect(x: x, y: y, width: width, height: height) }
}

struct VideoLayoutArgs: Decodable {
    let remote: VideoRectArgs?
    let local: VideoRectArgs?
    let mirrorLocal: Bool
    let localRadius: CGFloat
}

// width and height are only used on Android
struct VideoShapeArgs: Decodable {
    let width: UInt32
    let height: UInt32
    let rotation: Int
}

struct CallVideoArgs: Decodable {
    let on: Bool
}

// unretained, Rust owns the layer
func rustLayer(_ address: UInt64) -> CALayer? {
    guard address != 0, let pointer = UnsafeRawPointer(bitPattern: UInt(address)) else { return nil }
    return Unmanaged<CALayer>.fromOpaque(pointer).takeUnretainedValue()
}

final class VideoLayerHost: UIView {
    private(set) var hosted: CALayer?
    var rotation = 0 {
        didSet { if rotation != oldValue { setNeedsLayout() } }
    }

    func host(_ layer: CALayer?) {
        hosted?.removeFromSuperlayer()
        hosted = layer
        if let layer { self.layer.addSublayer(layer) }
        setNeedsLayout()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard let hosted else { return }
        let geometry = remoteLayerGeometry(room: bounds.size, rotation: rotation)
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        hosted.bounds = geometry.bounds
        hosted.position = geometry.position
        CATransaction.commit()
    }
}

final class FtVideoView: UIView {
    let remote = VideoLayerHost()
    let local = VideoLayerHost()
    var relayout: (() -> Void)?

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .black
        isUserInteractionEnabled = false
        autoresizingMask = [.flexibleWidth, .flexibleHeight]
        for host in [remote, local] {
            host.isHidden = true
            host.clipsToBounds = true
            addSubview(host)
        }
    }

    required init?(coder: NSCoder) {
        fatalError("not made from a storyboard")
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        relayout?()
    }
}

private struct WebViewLook {
    let opaque: Bool
    let background: UIColor?
    let scrollBackground: UIColor?
}

final class CallVideoViews {
    static let shared = CallVideoViews()

    private var video: FtVideoView?
    private weak var webView: WKWebView?
    private var look: WebViewLook?
    private var layout: VideoLayoutArgs?
    private var orientationObserver: NSObjectProtocol?

    var attached: Bool { video != nil }

    func attach(webView: WKWebView, remote: CALayer?, local: CALayer?) {
        if attached { detach() }
        guard let parent = webView.superview else { return }
        let view = FtVideoView(frame: webView.frame)
        view.remote.host(remote)
        view.local.host(local)
        view.relayout = { [weak self] in self?.place() }
        parent.insertSubview(view, belowSubview: webView)
        look = WebViewLook(opaque: webView.isOpaque, background: webView.backgroundColor, scrollBackground: webView.scrollView.backgroundColor)
        webView.isOpaque = false
        webView.backgroundColor = .clear
        webView.scrollView.backgroundColor = .clear
        video = view
        self.webView = webView
        place()
    }

    func layout(_ args: VideoLayoutArgs) {
        layout = args
        place()
    }

    func shape(rotation: Int) {
        video?.remote.rotation = rotation
    }

    // Rust may only release its video devices once the layers are out of the view hierarchy
    func detach() {
        stopOrientation()
        if let video {
            video.relayout = nil
            video.remote.host(nil)
            video.local.host(nil)
            video.removeFromSuperview()
        }
        if let webView, let look {
            webView.isOpaque = look.opaque
            webView.backgroundColor = look.background
            webView.scrollView.backgroundColor = look.scrollBackground
        }
        video = nil
        webView = nil
        look = nil
        layout = nil
    }

    private func place() {
        guard let video, let webView else { return }
        place(video.remote, css: layout?.remote?.rect, in: video, of: webView)
        place(video.local, css: layout?.local?.rect, in: video, of: webView)
        video.local.layer.cornerRadius = layout?.localRadius ?? 0
        mirrorPreview()
    }

    private func place(_ host: VideoLayerHost, css: CGRect?, in video: FtVideoView, of webView: WKWebView) {
        guard let css else {
            host.isHidden = true
            return
        }
        let scroll = webView.scrollView
        let rect = webViewRect(css: css, zoom: scroll.zoomScale, contentOffset: scroll.contentOffset)
        let frame = webView.convert(rect, to: video)
        if host.frame != frame { host.frame = frame }
        host.isHidden = false
    }

    private func mirrorPreview() {
        guard let layout,
              let preview = video?.local.hosted as? AVCaptureVideoPreviewLayer,
              let connection = preview.connection,
              let mirrored = previewMirrored(mirrorLocal: layout.mirrorLocal, supported: connection.isVideoMirroringSupported)
        else { return }
        if connection.automaticallyAdjustsVideoMirroring { connection.automaticallyAdjustsVideoMirroring = false }
        if connection.isVideoMirrored != mirrored { connection.isVideoMirrored = mirrored }
    }

    func startOrientation() {
        guard orientationObserver == nil else { return }
        let device = UIDevice.current
        device.beginGeneratingDeviceOrientationNotifications()
        orientationObserver = NotificationCenter.default.addObserver(forName: UIDevice.orientationDidChangeNotification, object: nil, queue: .main) { _ in
            CallEvents.shared.offer(.orientation(UIDevice.current.orientation.rawValue))
        }
        CallEvents.shared.offer(.orientation(device.orientation.rawValue))
    }

    private func stopOrientation() {
        guard let observer = orientationObserver else { return }
        NotificationCenter.default.removeObserver(observer)
        orientationObserver = nil
        UIDevice.current.endGeneratingDeviceOrientationNotifications()
    }
}

// resigning active (CallKit's screen, notification centre) isn't leaving, the app still shows
func visibilityEvent(_ name: Notification.Name) -> CallEvent? {
    switch name {
    case UIApplication.didEnterBackgroundNotification: return .visible(false)
    case UIApplication.didBecomeActiveNotification: return .visible(true)
    default: return nil
    }
}

// iOS suspends an app about 1.5 s after its CallKit call ends, before the core lets go of the router.
// Suspended first, the router keeps a dead socket and the next call doesn't ring.
func holdsAfterCall(_ state: UIApplication.State) -> Bool {
    state == .background
}

let afterCallHold: TimeInterval = 6

// iOS may suspend the app before the core has let go of the router
let leavingHold: TimeInterval = 3

enum AppVisibility {
    private static var observers: [NSObjectProtocol] = []

    static func install() {
        guard observers.isEmpty else { return }
        let center = NotificationCenter.default
        observers = [UIApplication.didBecomeActiveNotification, UIApplication.didEnterBackgroundNotification].map { name in
            center.addObserver(forName: name, object: nil, queue: .main) { note in
                guard let event = visibilityEvent(note.name) else { return }
                if event == .visible(false) { holdWhileLeaving() }
                CallEvents.shared.offer(event)
            }
        }
    }

    private static func holdWhileLeaving() {
        hold(leavingHold, name: "ft.leaving")
    }

    static func holdAfterCall() {
        guard holdsAfterCall(UIApplication.shared.applicationState) else { return }
        hold(afterCallHold, name: "ft.call.over")
    }

    private static func hold(_ seconds: TimeInterval, name: String) {
        var task = UIBackgroundTaskIdentifier.invalid
        let end = {
            guard task != .invalid else { return }
            UIApplication.shared.endBackgroundTask(task)
            task = .invalid
        }
        task = UIApplication.shared.beginBackgroundTask(withName: name, expirationHandler: end)
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds, execute: end)
    }

    static func tellNow() {
        CallEvents.shared.offer(.visible(appVisible(UIApplication.shared.applicationState)))
    }
}

// Tauri's app delegate and tao's scene delegate only look for web links, and tao reads nothing on scene
// connect, so their methods are wrapped to see the activity first. Unverified on a phone for CallKit's Video button.
enum CallIntentActivities {
    private static var installed = false

    static func install() {
        guard !installed else { return }
        installed = true
        if let scene: AnyClass = NSClassFromString("TaoSceneDelegate") {
            wrapSceneContinue(scene)
            wrapSceneConnect(scene)
        }
        if let delegate = UIApplication.shared.delegate {
            wrapAppContinue(type(of: delegate))
        }
    }

    @discardableResult
    static func heard(_ activity: NSUserActivity) -> Bool {
        if let link = openedLinkOf(activityType: activity.activityType, url: activity.webpageURL) {
            OpenedLinks.shared.put(link)
            return true
        }
        guard asksForVideo(activity) else { return false }
        CallEvents.shared.emit(.videoRequested)
        return true
    }

    private static func wrapSceneContinue(_ cls: AnyClass) {
        let selector = NSSelectorFromString("scene:continueUserActivity:")
        guard let method = class_getInstanceMethod(cls, selector) else { return }
        typealias Original = @convention(c) (AnyObject, Selector, AnyObject, NSUserActivity) -> Void
        let original = unsafeBitCast(method_getImplementation(method), to: Original.self)
        let wrapped: @convention(block) (AnyObject, AnyObject, NSUserActivity) -> Void = { this, scene, activity in
            heard(activity)
            original(this, selector, scene, activity)
        }
        method_setImplementation(method, imp_implementationWithBlock(wrapped))
    }

    // PushKit launched the app without a scene, and the call was answered on the lock screen
    private static func wrapSceneConnect(_ cls: AnyClass) {
        let selector = NSSelectorFromString("scene:willConnectToSession:options:")
        guard let method = class_getInstanceMethod(cls, selector) else { return }
        typealias Original = @convention(c) (AnyObject, Selector, UIScene, UISceneSession, UIScene.ConnectionOptions) -> Void
        let original = unsafeBitCast(method_getImplementation(method), to: Original.self)
        let wrapped: @convention(block) (AnyObject, UIScene, UISceneSession, UIScene.ConnectionOptions) -> Void = { this, scene, session, options in
            options.userActivities.forEach { heard($0) }
            original(this, selector, scene, session, options)
        }
        method_setImplementation(method, imp_implementationWithBlock(wrapped))
    }

    private static func wrapAppContinue(_ cls: AnyClass) {
        let selector = NSSelectorFromString("application:continueUserActivity:restorationHandler:")
        guard let method = class_getInstanceMethod(cls, selector) else { return }
        typealias Original = @convention(c) (AnyObject, Selector, UIApplication, NSUserActivity, AnyObject?) -> Bool
        let original = unsafeBitCast(method_getImplementation(method), to: Original.self)
        let wrapped: @convention(block) (AnyObject, UIApplication, NSUserActivity, AnyObject?) -> Bool = { this, app, activity, restore in
            let ours = heard(activity)
            return original(this, selector, app, activity, restore) || ours
        }
        method_setImplementation(method, imp_implementationWithBlock(wrapped))
    }
}
