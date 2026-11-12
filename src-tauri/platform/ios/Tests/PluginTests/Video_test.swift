import AVFoundation
import CallKit
import Intents
import UIKit
import WebKit
import XCTest
@testable import tauri_plugin_ft_platform

final class VideoTests: XCTestCase {
    func testCssPixelsArePointsOfTheWebView() {
        let rect = CGRect(x: 278, y: 594, width: 96, height: 140)
        XCTAssertEqual(webViewRect(css: rect, zoom: 1, contentOffset: .zero), rect)
    }

    func testTheSafeAreaInsetMovesTheRect() {
        let rect = CGRect(x: 0, y: 0, width: 390, height: 200)
        XCTAssertEqual(webViewRect(css: rect, zoom: 1, contentOffset: CGPoint(x: 0, y: -47)), CGRect(x: 0, y: 47, width: 390, height: 200))
        XCTAssertEqual(webViewRect(css: rect, zoom: 1, contentOffset: CGPoint(x: -59, y: 0)), CGRect(x: 59, y: 0, width: 390, height: 200))
    }

    func testTheZoomScalesTheRect() {
        XCTAssertEqual(webViewRect(css: CGRect(x: 10, y: 20, width: 30, height: 40), zoom: 2, contentOffset: .zero), CGRect(x: 20, y: 40, width: 60, height: 80))
    }

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

    func testTheLayoutIsReadAsRustSendsIt() throws {
        let json = #"{"remote":null,"local":{"x":278,"y":594,"width":96,"height":140},"mirrorLocal":true,"localRadius":16}"#
        let layout = try JSONDecoder().decode(VideoLayoutArgs.self, from: Data(json.utf8))
        XCTAssertNil(layout.remote)
        XCTAssertEqual(layout.local?.rect, CGRect(x: 278, y: 594, width: 96, height: 140))
        XCTAssertTrue(layout.mirrorLocal)
        XCTAssertEqual(layout.localRadius, 16)
    }

    func testTheCommandsAreReadAsRustSendsThem() throws {
        let attach = try JSONDecoder().decode(AttachVideoArgs.self, from: Data(#"{"remoteLayer":4096,"localLayer":8192}"#.utf8))
        XCTAssertEqual(attach.remoteLayer, 4096)
        XCTAssertEqual(attach.localLayer, 8192)
        let shape = try JSONDecoder().decode(VideoShapeArgs.self, from: Data(#"{"width":480,"height":640,"rotation":90}"#.utf8))
        XCTAssertEqual(shape.rotation, 90)
        XCTAssertTrue(try JSONDecoder().decode(CallVideoArgs.self, from: Data(#"{"on":true}"#.utf8)).on)
        let layout = try JSONDecoder().decode(VideoLayoutArgs.self, from: Data(#"{"remote":null,"local":null,"mirrorLocal":false,"localRadius":0}"#.utf8))
        XCTAssertEqual(layout.localRadius, 0)
    }

    func testThePreviewIsMirroredOnlyForTheFrontCamera() {
        XCTAssertEqual(previewMirrored(mirrorLocal: true, supported: true), true)
        XCTAssertEqual(previewMirrored(mirrorLocal: false, supported: true), false)
        XCTAssertNil(previewMirrored(mirrorLocal: true, supported: false))
    }

    func testTheCameraIsAskedOnlyWhenItWasNeverAsked() {
        XCTAssertEqual(cameraAccess(.authorized), .granted)
        XCTAssertEqual(cameraAccess(.notDetermined), .ask)
        XCTAssertEqual(cameraAccess(.denied), .denied)
        XCTAssertEqual(cameraAccess(.restricted), .denied)
    }

    func testTheAppIsVisibleUnlessInTheBackground() {
        XCTAssertTrue(appVisible(.active))
        XCTAssertTrue(appVisible(.inactive))
        XCTAssertFalse(appVisible(.background))
    }

    func testTheLifecycleSaysWhenTheAppLeavesTheForegroundAndComesBack() {
        XCTAssertEqual(visibilityEvent(UIApplication.didEnterBackgroundNotification), .visible(false))
        XCTAssertEqual(visibilityEvent(UIApplication.didBecomeActiveNotification), .visible(true))
        XCTAssertNil(visibilityEvent(UIApplication.willResignActiveNotification))
        XCTAssertNil(visibilityEvent(UIApplication.willEnterForegroundNotification))
    }

    func testACallThatEndsInTheBackgroundHoldsTheAppUntilTheCoreLetsGo() {
        XCTAssertTrue(holdsAfterCall(.background))
        XCTAssertFalse(holdsAfterCall(.active), "in front, the app stays connected")
        XCTAssertFalse(holdsAfterCall(.inactive), "still on the screen (CallKit's own over it)")
        XCTAssertGreaterThan(afterCallHold, 3 + 1)
    }

    func testVideoEventsTravelAsTheCoreReadsThem() {
        XCTAssertEqual(callEventPayload(.visible(true)) as NSDictionary, ["event": "visible", "visible": true] as NSDictionary)
        XCTAssertEqual(callEventPayload(.visible(false)) as NSDictionary, ["event": "visible", "visible": false] as NSDictionary)
        XCTAssertEqual(callEventPayload(.orientation(3)) as NSDictionary, ["event": "orientation", "orientation": 3] as NSDictionary)
        XCTAssertEqual(callEventPayload(.videoRequested) as NSDictionary, ["event": "video"] as NSDictionary)
    }

    func testAnOfferReachesOnlyACoreThatListens() {
        let queue = CallEventQueue()
        XCTAssertFalse(queue.offer(.visible(false)))
        var heard: [CallEvent] = []
        queue.register { heard.append($0) }
        XCTAssertEqual(heard, [])
        XCTAssertTrue(queue.offer(.orientation(1)))
        XCTAssertEqual(heard, [.orientation(1)])
    }

    func testCallKitHearsOnlyWhetherTheCallHasVideo() {
        let on = videoUpdate(true)
        XCTAssertTrue(on.hasVideo)
        XCTAssertNil(on.remoteHandle)
        XCTAssertNil(on.localizedCallerName)
        XCTAssertFalse(videoUpdate(false).hasVideo)
    }

    func testOnlyAVideoCallIntentAsksForVideo() {
        XCTAssertTrue(asksForVideo(activityType: "INStartVideoCallIntent", capability: nil))
        XCTAssertTrue(asksForVideo(activityType: "INStartCallIntent", capability: .videoCall))
        XCTAssertFalse(asksForVideo(activityType: "INStartCallIntent", capability: .audioCall))
        XCTAssertFalse(asksForVideo(activityType: "INStartCallIntent", capability: nil))
        XCTAssertFalse(asksForVideo(activityType: "INStartAudioCallIntent", capability: nil))
        XCTAssertFalse(asksForVideo(activityType: NSUserActivityTypeBrowsingWeb, capability: nil))
    }

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
