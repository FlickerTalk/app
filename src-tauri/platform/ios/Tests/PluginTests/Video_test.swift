import AVFoundation
import CallKit
import Intents
import UIKit
import WebKit
import XCTest
@testable import tauri_plugin_ft_platform

// Native video (2026-09-29, docs/video-nativo.md): the engine's layers under a transparent
// WebView, placed where the WebView leaves room for them; CallKit's `hasVideo`, the camera
// permission, and the events the core holds our camera by.
final class VideoTests: XCTestCase {
    // At the WebView's zoom of 1, one CSS pixel is one UIKit point, measured from where the page's
    // viewport starts: the scroll view's offset says where that is in the WebView.
    func testCssPixelsArePointsOfTheWebView() {
        let rect = CGRect(x: 278, y: 594, width: 96, height: 140)
        XCTAssertEqual(webViewRect(css: rect, zoom: 1, contentOffset: .zero), rect)
    }

    // Safe areas: a WebView that keeps its content out of the notch has the content moved down by
    // the inset, and the scroll view's offset says so (-inset at rest).
    func testTheSafeAreaInsetMovesTheRect() {
        let rect = CGRect(x: 0, y: 0, width: 390, height: 200)
        XCTAssertEqual(webViewRect(css: rect, zoom: 1, contentOffset: CGPoint(x: 0, y: -47)), CGRect(x: 0, y: 47, width: 390, height: 200))
        XCTAssertEqual(webViewRect(css: rect, zoom: 1, contentOffset: CGPoint(x: -59, y: 0)), CGRect(x: 59, y: 0, width: 390, height: 200))
    }

    // A zoomed WebView (it never is: `user-scalable=no`) would scale the rect too.
    func testTheZoomScalesTheRect() {
        XCTAssertEqual(webViewRect(css: CGRect(x: 10, y: 20, width: 30, height: 40), zoom: 2, contentOffset: .zero), CGRect(x: 20, y: 40, width: 60, height: 80))
    }

    // The remote layer keeps the picture's aspect ratio by itself; the engine turns it with its
    // transform, so it is laid out with bounds and position (never `frame`), and a quarter turn
    // swaps width and height.
    func testTheRemoteLayerFillsItsRoomUnderItsTurn() {
        let room = CGSize(width: 390, height: 844)
        let upright = remoteLayerGeometry(room: room, rotation: 0)
        XCTAssertEqual(upright.bounds, CGRect(x: 0, y: 0, width: 390, height: 844))
        XCTAssertEqual(upright.position, CGPoint(x: 195, y: 422))
        for rotation in [90, 270] {
            let turned = remoteLayerGeometry(room: room, rotation: rotation)
            XCTAssertEqual(turned.bounds, CGRect(x: 0, y: 0, width: 844, height: 390), "rotation \(rotation)")
            XCTAssertEqual(turned.position, CGPoint(x: 195, y: 422), "rotation \(rotation)")
        }
        XCTAssertEqual(remoteLayerGeometry(room: room, rotation: 180).bounds.size, room)
    }

    // What the WebView sends (`VideoLayout` in Rust), hidden pictures as null.
    func testTheLayoutIsReadAsRustSendsIt() throws {
        let json = #"{"remote":null,"local":{"x":278,"y":594,"width":96,"height":140},"mirrorLocal":true,"localRadius":16}"#
        let layout = try JSONDecoder().decode(VideoLayoutArgs.self, from: Data(json.utf8))
        XCTAssertNil(layout.remote)
        XCTAssertEqual(layout.local?.rect, CGRect(x: 278, y: 594, width: 96, height: 140))
        XCTAssertTrue(layout.mirrorLocal)
        XCTAssertEqual(layout.localRadius, 16)
    }

    // The other commands' arguments, as Rust sends them (`AttachVideo`, `VideoShape`, `CallVideo`).
    func testTheCommandsAreReadAsRustSendsThem() throws {
        let attach = try JSONDecoder().decode(AttachVideoArgs.self, from: Data(#"{"remoteLayer":4096,"localLayer":8192}"#.utf8))
        XCTAssertEqual(attach.remoteLayer, 4096)
        XCTAssertEqual(attach.localLayer, 8192)
        let shape = try JSONDecoder().decode(VideoShapeArgs.self, from: Data(#"{"width":480,"height":640,"rotation":90}"#.utf8))
        XCTAssertEqual(shape.rotation, 90)
        XCTAssertTrue(try JSONDecoder().decode(CallVideoArgs.self, from: Data(#"{"on":true}"#.utf8)).on)
        // The WebView may write whole numbers for the radius.
        let layout = try JSONDecoder().decode(VideoLayoutArgs.self, from: Data(#"{"remote":null,"local":null,"mirrorLocal":false,"localRadius":0}"#.utf8))
        XCTAssertEqual(layout.localRadius, 0)
    }

    // The camera is flipped in Rust; here our preview only follows the facing the layout carries:
    // mirrored for the front camera, as the user expects, and never for the back one. A connection
    // that cannot mirror is left alone.
    func testThePreviewIsMirroredOnlyForTheFrontCamera() {
        XCTAssertEqual(previewMirrored(mirrorLocal: true, supported: true), true)
        XCTAssertEqual(previewMirrored(mirrorLocal: false, supported: true), false)
        XCTAssertNil(previewMirrored(mirrorLocal: true, supported: false))
    }

    // The camera is asked for when it is turned on (§30): allowed answers at once, never asked
    // asks, and a no (or a restriction) stays a no.
    func testTheCameraIsAskedOnlyWhenItWasNeverAsked() {
        XCTAssertEqual(cameraAccess(.authorized), .granted)
        XCTAssertEqual(cameraAccess(.notDetermined), .ask)
        XCTAssertEqual(cameraAccess(.denied), .denied)
        XCTAssertEqual(cameraAccess(.restricted), .denied)
    }

    // The app is on the screen unless it is in the background: iOS stops the camera there.
    func testTheAppIsVisibleUnlessInTheBackground() {
        XCTAssertTrue(appVisible(.active))
        XCTAssertTrue(appVisible(.inactive))
        XCTAssertFalse(appVisible(.background))
    }

    // 2026-10-01: entering the background tells the core the app left the foreground (it lets go
    // of the router, so the router pushes what comes); becoming active, that it is back. Resigning
    // active (CallKit's screen over the app, the notification centre pulled down) is neither: the
    // app is still on the screen.
    func testTheLifecycleSaysWhenTheAppLeavesTheForegroundAndComesBack() {
        XCTAssertEqual(visibilityEvent(UIApplication.didEnterBackgroundNotification), .visible(false))
        XCTAssertEqual(visibilityEvent(UIApplication.didBecomeActiveNotification), .visible(true))
        XCTAssertNil(visibilityEvent(UIApplication.willResignActiveNotification))
        XCTAssertNil(visibilityEvent(UIApplication.willEnterForegroundNotification))
    }

    // Native video's events, as Rust's `NativeCallEvent` reads them.
    func testVideoEventsTravelAsTheCoreReadsThem() {
        XCTAssertEqual(callEventPayload(.visible(true)) as NSDictionary, ["event": "visible", "visible": true] as NSDictionary)
        XCTAssertEqual(callEventPayload(.visible(false)) as NSDictionary, ["event": "visible", "visible": false] as NSDictionary)
        XCTAssertEqual(callEventPayload(.orientation(3)) as NSDictionary, ["event": "orientation", "orientation": 3] as NSDictionary)
        XCTAssertEqual(callEventPayload(.videoRequested) as NSDictionary, ["event": "video"] as NSDictionary)
    }

    // Visibility and orientation only matter to a core that listens now: nothing waits for a
    // later one (it hears the app's visibility when it registers).
    func testAnOfferReachesOnlyACoreThatListens() {
        let queue = CallEventQueue()
        XCTAssertFalse(queue.offer(.visible(false)))
        var heard: [CallEvent] = []
        queue.register { heard.append($0) }
        XCTAssertEqual(heard, [])
        XCTAssertTrue(queue.offer(.orientation(1)))
        XCTAssertEqual(heard, [.orientation(1)])
    }

    // CallKit learns whether the call has video, and nothing else changes: the caller's name stays.
    func testCallKitHearsOnlyWhetherTheCallHasVideo() {
        let on = videoUpdate(true)
        XCTAssertTrue(on.hasVideo)
        XCTAssertNil(on.remoteHandle)
        XCTAssertNil(on.localizedCallerName)
        XCTAssertFalse(videoUpdate(false).hasVideo)
    }

    // CallKit's "Video" button opens the app with a call intent's user activity: a video one
    // turns our camera on; anything else only opens the app.
    func testOnlyAVideoCallIntentAsksForVideo() {
        XCTAssertTrue(asksForVideo(activityType: "INStartVideoCallIntent", capability: nil))
        XCTAssertTrue(asksForVideo(activityType: "INStartCallIntent", capability: .videoCall))
        XCTAssertFalse(asksForVideo(activityType: "INStartCallIntent", capability: .audioCall))
        XCTAssertFalse(asksForVideo(activityType: "INStartCallIntent", capability: nil))
        XCTAssertFalse(asksForVideo(activityType: "INStartAudioCallIntent", capability: nil))
        XCTAssertFalse(asksForVideo(activityType: NSUserActivityTypeBrowsingWeb, capability: nil))
    }

    // The views go under the WebView, which turns transparent (and its scroll view too) while
    // video shows, and all of it comes back as it was.
    func testTheViewsGoUnderATransparentWebViewAndLeaveNothingBehind() {
        let root = UIView(frame: CGRect(x: 0, y: 0, width: 390, height: 844))
        let web = WKWebView(frame: root.bounds)
        web.backgroundColor = .white
        web.scrollView.backgroundColor = .white
        root.addSubview(web)
        let remote = CALayer()
        let local = CALayer()
        let views = CallVideoViews()

        views.attach(webView: web, remote: remote, local: local)
        XCTAssertEqual(root.subviews.count, 2)
        XCTAssertTrue(root.subviews.first is FtVideoView)
        XCTAssertIdentical(root.subviews.last, web)
        XCTAssertFalse(web.isOpaque)
        XCTAssertEqual(web.backgroundColor, .clear)
        XCTAssertEqual(web.scrollView.backgroundColor, .clear)
        XCTAssertNotNil(remote.superlayer)
        XCTAssertNotNil(local.superlayer)
        XCTAssertFalse(root.subviews.first!.isUserInteractionEnabled)

        views.detach()
        XCTAssertEqual(root.subviews, [web])
        XCTAssertNil(remote.superlayer)
        XCTAssertNil(local.superlayer)
        XCTAssertTrue(web.isOpaque)
        XCTAssertEqual(web.backgroundColor, .white)
        XCTAssertEqual(web.scrollView.backgroundColor, .white)
    }

    // Each picture goes where the WebView left room for it; null hides it. Ours has its corners,
    // and the other side's layer turns a quarter under its transform with width and height swapped.
    func testTheLayoutPlacesTheViews() throws {
        let root = UIView(frame: CGRect(x: 0, y: 0, width: 390, height: 844))
        let web = WKWebView(frame: root.bounds)
        root.addSubview(web)
        let remote = CALayer()
        let local = CALayer()
        let views = CallVideoViews()
        views.attach(webView: web, remote: remote, local: local)
        let video = try XCTUnwrap(root.subviews.first as? FtVideoView)

        let json = #"{"remote":{"x":0,"y":0,"width":390,"height":844},"local":{"x":278,"y":594,"width":96,"height":140},"mirrorLocal":true,"localRadius":16}"#
        views.layout(try JSONDecoder().decode(VideoLayoutArgs.self, from: Data(json.utf8)))
        views.shape(rotation: 90)
        video.layoutIfNeeded()
        XCTAssertFalse(video.remote.isHidden)
        XCTAssertEqual(video.remote.frame, CGRect(x: 0, y: 0, width: 390, height: 844))
        XCTAssertEqual(remote.bounds, CGRect(x: 0, y: 0, width: 844, height: 390))
        XCTAssertEqual(remote.position, CGPoint(x: 195, y: 422))
        XCTAssertEqual(video.local.frame, CGRect(x: 278, y: 594, width: 96, height: 140))
        XCTAssertEqual(video.local.layer.cornerRadius, 16)
        XCTAssertTrue(video.local.clipsToBounds)
        XCTAssertEqual(local.frame, CGRect(x: 0, y: 0, width: 96, height: 140))

        views.layout(try JSONDecoder().decode(VideoLayoutArgs.self, from: Data(#"{"remote":null,"local":null,"mirrorLocal":false,"localRadius":0}"#.utf8)))
        XCTAssertTrue(video.remote.isHidden)
        XCTAssertTrue(video.local.isHidden)
        views.detach()
    }

    // The app declares the call intents' user activities, so the system hands them over.
    func testTheAppDeclaresTheCallActivities() throws {
        let plist = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("gen/apple/flickertalk_iOS/Info.plist")
        let info = try XCTUnwrap(PropertyListSerialization.propertyList(from: Data(contentsOf: plist), format: nil) as? [String: Any])
        let types = try XCTUnwrap(info["NSUserActivityTypes"] as? [String])
        XCTAssertTrue(types.contains("INStartCallIntent"))
        XCTAssertTrue(types.contains("INStartVideoCallIntent"))
        XCTAssertFalse((info["NSCameraUsageDescription"] as? String ?? "").isEmpty)
    }
}
