import AVFoundation
import CallKit
import Intents
import ObjectiveC
import UIKit
import WebKit

// ---- Native video (2026-09-29, docs/video-nativo.md) ----
// The call's pictures live in Rust (`ft-media` over the engine's `CameraSource` and
// `DisplaySink`), which owns their layers: the remote `AVSampleBufferDisplayLayer` and our
// `AVCaptureVideoPreviewLayer`. Here they get their place on the screen: under the WebView, which
// turns transparent while video shows and keeps every control (HTML, with its i18n and
// accessibility) and every touch. All the layer work happens on the main thread.

/// A rectangle of the WebView's page (CSS pixels, `getBoundingClientRect`) in the WebView's own
/// points. At the zoom of 1 (`user-scalable=no`) a CSS pixel is a UIKit point, measured from where
/// the page's viewport starts; the scroll view's offset says where that is: `-inset` at rest when
/// UIKit keeps the page out of the safe areas, 0 with `viewport-fit=cover` and no inset. The page
/// itself never scrolls (Ionic scrolls inside `ion-content`), so the offset is only that shift.
func webViewRect(css: CGRect, zoom: CGFloat, contentOffset: CGPoint) -> CGRect {
    CGRect(
        x: css.origin.x * zoom - contentOffset.x,
        y: css.origin.y * zoom - contentOffset.y,
        width: css.width * zoom,
        height: css.height * zoom
    )
}

/// Where the other side's layer goes in its room: the engine turns it with its transform (the
/// frame's clockwise rotation), so it is laid out with `bounds` and `position`, never `frame`
/// (undefined under a transform), with width and height swapped for a quarter turn. The layer
/// keeps the picture's aspect ratio by itself (`resizeAspect`).
func remoteLayerGeometry(room: CGSize, rotation: Int) -> (bounds: CGRect, position: CGPoint) {
    let quarter = rotation % 180 != 0
    let size = quarter ? CGSize(width: room.height, height: room.width) : room
    return (CGRect(origin: .zero, size: size), CGPoint(x: room.width / 2, y: room.height / 2))
}

/// Whether our preview is shown as in a mirror: exactly when the WebView says so (the front
/// camera; the camera is switched in Rust, and the WebView's layout carries the facing). `nil`
/// when the preview's connection cannot mirror: it is left as it is. AVFoundation already mirrors
/// the front camera by itself; setting it here keeps the preview right whatever the connection
/// was left with, and a connection made anew by a camera switch starts from that default again.
func previewMirrored(mirrorLocal: Bool, supported: Bool) -> Bool? {
    supported ? mirrorLocal : nil
}

/// What `requestCamera` does with the camera's authorisation.
enum CameraAccess: Equatable {
    case granted
    case ask
    case denied
}

/// The camera is asked for when it is turned on (§30): allowed answers at once, never asked asks,
/// and a no (or a restriction, such as Screen Time) stays a no.
func cameraAccess(_ status: AVAuthorizationStatus) -> CameraAccess {
    switch status {
    case .authorized: return .granted
    case .notDetermined: return .ask
    case .denied, .restricted: return .denied
    @unknown default: return .denied
    }
}

/// The app is on the screen unless it is in the background: iOS stops the camera there, and the
/// core holds ours meanwhile (`NativeCallEvent::Visible`). Inactive (CallKit's screen over the
/// app, the notification centre pulled down) still shows it.
func appVisible(_ state: UIApplication.State) -> Bool {
    state != .background
}

/// What CallKit hears when the call gains or loses video: only `hasVideo`. A `CXCallUpdate` sends
/// only what was set, so the caller's name and the rest stay as they were.
func videoUpdate(_ on: Bool) -> CXCallUpdate {
    let update = CXCallUpdate()
    update.hasVideo = on
    return update
}

/// Whether a user activity asks for video: CallKit's "Video" button opens the app with a call
/// intent (`INStartCallIntent` with the video capability, or the older `INStartVideoCallIntent`).
/// Anything else only opens the app.
func asksForVideo(activityType: String, capability: INCallCapability?) -> Bool {
    switch activityType {
    case "INStartVideoCallIntent": return true
    case "INStartCallIntent": return capability == .videoCall
    default: return false
    }
}

/// The same for a user activity as the system hands it over.
func asksForVideo(_ activity: NSUserActivity) -> Bool {
    let intent = activity.interaction?.intent as? INStartCallIntent
    return asksForVideo(activityType: activity.activityType, capability: intent?.callCapability)
}

/// Arguments of `attachVideo`: the addresses of the call's `CALayer`s (`ft_media::views::layers`),
/// owned by the Rust objects; 0 when there is none.
struct AttachVideoArgs: Decodable {
    let remoteLayer: UInt64
    let localLayer: UInt64
}

/// A rectangle of the WebView in CSS pixels, from its top left corner.
struct VideoRectArgs: Decodable {
    let x: CGFloat
    let y: CGFloat
    let width: CGFloat
    let height: CGFloat

    var rect: CGRect { CGRect(x: x, y: y, width: width, height: height) }
}

/// Arguments of `videoLayout` (`VideoLayout` in Rust): where the WebView leaves room for each
/// picture (`null` hides it), whether our preview is mirrored (the front camera) and its corners.
struct VideoLayoutArgs: Decodable {
    let remote: VideoRectArgs?
    let local: VideoRectArgs?
    let mirrorLocal: Bool
    let localRadius: CGFloat
}

/// Arguments of `videoShape`: the turn the remote layer's transform applies (iOS lays the layer
/// out by it; the size is Android's).
struct VideoShapeArgs: Decodable {
    let width: UInt32
    let height: UInt32
    let rotation: Int
}

/// Arguments of `callVideo`: whether the call has video now (either camera on).
struct CallVideoArgs: Decodable {
    let on: Bool
}

/// The layer of `address`, as Rust owns it (`Unmanaged`, not retained by the cast itself), or nil.
func rustLayer(_ address: UInt64) -> CALayer? {
    guard address != 0, let pointer = UnsafeRawPointer(bitPattern: UInt(address)) else { return nil }
    return Unmanaged<CALayer>.fromOpaque(pointer).takeUnretainedValue()
}

/// A view that shows one of the engine's layers, laid out in `layoutSubviews`.
final class VideoLayerHost: UIView {
    private(set) var hosted: CALayer?
    /// The clockwise turn the hosted layer's own transform applies (the remote picture).
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

/// The call's pictures under the WebView: the other side's, and ours over it with its corners.
/// Never touched: the WebView above takes every touch.
final class FtVideoView: UIView {
    let remote = VideoLayerHost()
    let local = VideoLayerHost()
    /// Places the pictures again when this view changes size (the phone turned).
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

/// How the WebView looked before video: given back as it was.
private struct WebViewLook {
    let opaque: Bool
    let background: UIColor?
    let scrollBackground: UIColor?
}

/// The call's video views, from `attachVideo` to `detachVideo`. Main thread only.
final class CallVideoViews {
    static let shared = CallVideoViews()

    private var video: FtVideoView?
    private weak var webView: WKWebView?
    private var look: WebViewLook?
    private var layout: VideoLayoutArgs?
    private var orientationObserver: NSObjectProtocol?

    var attached: Bool { video != nil }

    /// Puts the layers under `webView`, hidden until `layout` places them, and makes the WebView
    /// transparent. A second attach replaces the first.
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

    /// Takes the layers out of the view hierarchy (only then may Rust let its video devices go)
    /// and gives the WebView its look back.
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

    /// Our preview mirrored for the front camera only (`previewMirrored`).
    private func mirrorPreview() {
        guard let layout,
              let preview = video?.local.hosted as? AVCaptureVideoPreviewLayer,
              let connection = preview.connection,
              let mirrored = previewMirrored(mirrorLocal: layout.mirrorLocal, supported: connection.isVideoMirroringSupported)
        else { return }
        if connection.automaticallyAdjustsVideoMirroring { connection.automaticallyAdjustsVideoMirroring = false }
        if connection.isVideoMirrored != mirrored { connection.isVideoMirrored = mirrored }
    }

    // Orientation: only while video is attached, for the rotation our frames carry (the engine's
    // `set_device_orientation`, through `NativeCallEvent::Orientation`).

    /// Starts telling the core the phone's orientation, now and on each turn.
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

/// What a lifecycle notification tells the core (2026-10-01): entering the background, the app
/// left the foreground (the core holds our camera and lets go of the router, so that the router
/// pushes what comes); becoming active, it is back. Resigning active (CallKit's screen over the
/// app, the notification centre pulled down) is neither: the app is still on the screen.
func visibilityEvent(_ name: Notification.Name) -> CallEvent? {
    switch name {
    case UIApplication.didEnterBackgroundNotification: return .visible(false)
    case UIApplication.didBecomeActiveNotification: return .visible(true)
    default: return nil
    }
}

/// Whether a call that ends now holds the app a while (2026-10-01): out of the foreground, the
/// core lets go of the router a moment after the call (`LINGER`, 3 s, then its next look, 1 s),
/// and iOS suspends an app whose CallKit call ended within about 1.5 s. Suspended first, it would
/// leave the router a socket nobody reads, and the next call would not ring.
func holdsAfterCall(_ state: UIApplication.State) -> Bool {
    state == .background
}

/// How long a call that ended out of the foreground holds the app.
let afterCallHold: TimeInterval = 6

/// How long the app asks iOS to keep running after it enters the background: the core lets go of
/// the router and the other phones in milliseconds, but iOS may suspend the app sooner than that.
let leavingHold: TimeInterval = 3

/// Whether the app is on the screen, for the core (`NativeCallEvent::Visible`): the camera only
/// runs with the app in front, and out of the foreground the core lets go of the router
/// (2026-10-01). Installed when the plugin loads; main thread.
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

    /// A moment of background time, so that the core has let go before iOS suspends the app.
    private static func holdWhileLeaving() {
        hold(leavingHold, name: "ft.leaving")
    }

    /// A call ended: out of the foreground, time for the core to let go after it. Main thread.
    static func holdAfterCall() {
        guard holdsAfterCall(UIApplication.shared.applicationState) else { return }
        hold(afterCallHold, name: "ft.call.over")
    }

    /// Asks iOS to keep the app running `seconds` more. Main thread.
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

    /// The core registered: it hears where the app is now.
    static func tellNow() {
        CallEvents.shared.offer(.visible(appVisible(UIApplication.shared.applicationState)))
    }
}

/// CallKit's "Video" button (2026-09-29): it opens the app with a call intent's user activity.
/// Tauri's app delegate and tao's scene delegate (`TaoSceneDelegate`) receive it and only look
/// for a web link, so their methods are wrapped once: ours hears the activity first, then theirs
/// runs as before. A video one tells the core (`VideoRequested`), which turns our camera on.
/// Unverified on a phone: if the activity does not reach these methods, the button only opens the
/// app, and the user turns the camera on in it.
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

    static func heard(_ activity: NSUserActivity) {
        guard asksForVideo(activity) else { return }
        CallEvents.shared.emit(.videoRequested)
    }

    /// `scene:continueUserActivity:`: the app was running with its scene.
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

    /// `scene:willConnectToSession:options:`: the scene comes with the activity (an app PushKit
    /// launched without a scene, answered on the lock screen).
    private static func wrapSceneConnect(_ cls: AnyClass) {
        let selector = NSSelectorFromString("scene:willConnectToSession:options:")
        guard let method = class_getInstanceMethod(cls, selector) else { return }
        typealias Original = @convention(c) (AnyObject, Selector, UIScene, UISceneSession, UIScene.ConnectionOptions) -> Void
        let original = unsafeBitCast(method_getImplementation(method), to: Original.self)
        let wrapped: @convention(block) (AnyObject, UIScene, UISceneSession, UIScene.ConnectionOptions) -> Void = { this, scene, session, options in
            options.userActivities.forEach(heard)
            original(this, selector, scene, session, options)
        }
        method_setImplementation(method, imp_implementationWithBlock(wrapped))
    }

    /// `application:continueUserActivity:restorationHandler:`, for an app without scenes.
    private static func wrapAppContinue(_ cls: AnyClass) {
        let selector = NSSelectorFromString("application:continueUserActivity:restorationHandler:")
        guard let method = class_getInstanceMethod(cls, selector) else { return }
        typealias Original = @convention(c) (AnyObject, Selector, UIApplication, NSUserActivity, AnyObject?) -> Bool
        let original = unsafeBitCast(method_getImplementation(method), to: Original.self)
        let wrapped: @convention(block) (AnyObject, UIApplication, NSUserActivity, AnyObject?) -> Bool = { this, app, activity, restore in
            heard(activity)
            return original(this, selector, app, activity, restore) || asksForVideo(activity)
        }
        method_setImplementation(method, imp_implementationWithBlock(wrapped))
    }
}
