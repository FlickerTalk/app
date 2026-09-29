package com.flickertalk.platform

import android.content.pm.ServiceInfo
import android.view.Surface
import app.tauri.PermissionState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

// Native video (2026-09-29, docs/video-nativo.md): the call's pictures in SurfaceViews behind a
// transparent WebView, their surfaces handed to Rust, the camera permission and the service type.
class FtVideoTest {
    // The WebView measures in CSS pixels; a view is laid out in the screen's pixels.
    @Test
    fun cssPixelsBecomeScreenPixels() {
        assertEquals(PixelRect(0, 0, 1080, 2340), cssToPixels(CssRect(0.0, 0.0, 360.0, 780.0), 3f))
        assertEquals(PixelRect(834, 1782, 288, 420), cssToPixels(CssRect(278.0, 594.0, 96.0, 140.0), 3f))
    }

    // Fractions of a CSS pixel round by the edges, so two rects that touch leave no gap.
    @Test
    fun fractionsRoundByTheEdges() {
        val left = cssToPixels(CssRect(0.0, 0.0, 100.3, 50.0), 2.625f)
        val right = cssToPixels(CssRect(100.3, 0.0, 100.3, 50.0), 2.625f)
        assertEquals(left.left + left.width, right.left)
        assertEquals(263, left.width)
    }

    // The views sit in the WebView's parent: where the WebView starts there (not under the system
    // bars, if the app were not edge to edge) moves them too.
    @Test
    fun theWebViewsOwnPlaceMovesTheViews() {
        assertEquals(PixelRect(10, 110, 300, 300), cssToPixels(CssRect(0.0, 0.0, 100.0, 100.0), 3f, offsetX = 10, offsetY = 110))
    }

    // `null` hides a view without letting its surface go: it waits off the screen, one pixel big.
    @Test
    fun aHiddenViewWaitsOffTheScreen() {
        val parked = viewPlacement(null, 3f)
        assertTrue(parked.left + parked.width <= 0 && parked.top + parked.height <= 0)
        assertTrue(parked.width > 0 && parked.height > 0)
        assertEquals(PixelRect(30, 60, 90, 120), viewPlacement(CssRect(10.0, 20.0, 30.0, 40.0), 3f))
    }

    // A SurfaceView stretches what it shows: the other side's picture is fitted whole in its room,
    // centred, as it is shaped.
    @Test
    fun theRemotePictureKeepsItsShape() {
        val room = PixelRect(0, 0, 1080, 2340)
        assertEquals(PixelRect(0, 765, 1080, 810), fitPicture(room, 640, 480))
        assertEquals(PixelRect(0, 210, 1080, 1920), fitPicture(room, 720, 1280))
        assertEquals(PixelRect(40, 0, 1000, 500), fitPicture(PixelRect(0, 0, 1080, 500), 1280, 640))
    }

    // Until the picture's size is known, it fills its room.
    @Test
    fun aPictureOfUnknownSizeFillsItsRoom() {
        val room = PixelRect(5, 6, 700, 900)
        assertEquals(room, fitPicture(room, 0, 0))
        assertEquals(room, fitPicture(room, 640, 0))
    }

    // What Kotlin passes to Rust over JNI: 0 the other side's picture, 1 our preview.
    @Test
    fun theSurfaceSlotsMatchRust() {
        assertEquals(0, FtVideoSurfaces.REMOTE)
        assertEquals(1, FtVideoSurfaces.LOCAL)
    }

    // The core takes the display's rotation in degrees (`CameraSource::set_display_rotation`).
    @Test
    fun theDisplayRotationIsInDegrees() {
        assertEquals(0, rotationDegrees(Surface.ROTATION_0))
        assertEquals(90, rotationDegrees(Surface.ROTATION_90))
        assertEquals(180, rotationDegrees(Surface.ROTATION_180))
        assertEquals(270, rotationDegrees(Surface.ROTATION_270))
    }

    // Native video's events, as Rust's `NativeCallEvent` reads them.
    @Test
    fun videoEventsTravelAsTheCoreReadsThem() {
        assertEquals(mapOf("event" to "visible", "visible" to true), callEventPayload(CallEvent.Visible(true)))
        assertEquals(mapOf("event" to "visible", "visible" to false), callEventPayload(CallEvent.Visible(false)))
        assertEquals(mapOf("event" to "orientation", "orientation" to 270), callEventPayload(CallEvent.Orientation(270)))
        assertEquals(mapOf("event" to "video"), callEventPayload(CallEvent.VideoRequested))
    }

    // The camera is asked for when it is turned on (§30): allowed is an answer at once, anything
    // else asks the system (which answers at once itself if the user said "never"); a build that
    // does not declare it is a no.
    @Test
    fun theCameraPermissionIsAskedOnlyWhenItIsNotAllowed() {
        assertEquals(CameraRequest.GRANTED, cameraRequest(PermissionState.GRANTED))
        assertEquals(CameraRequest.ASK, cameraRequest(PermissionState.PROMPT))
        assertEquals(CameraRequest.ASK, cameraRequest(PermissionState.PROMPT_WITH_RATIONALE))
        assertEquals(CameraRequest.ASK, cameraRequest(PermissionState.DENIED))
        assertEquals(CameraRequest.DENIED, cameraRequest(null))
    }

    // The call's service holds the camera type while the call has video (Android 11 and up),
    // next to phone call and microphone.
    @Test
    fun theCallServiceHoldsTheCameraWhileTheCallHasVideo() {
        val call = ServiceInfo.FOREGROUND_SERVICE_TYPE_PHONE_CALL
        val microphone = ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
        val camera = ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA
        assertEquals(call or microphone or camera, callServiceTypes(34, microphone = true, camera = true))
        assertEquals(call or camera, callServiceTypes(34, microphone = false, camera = true))
        assertEquals(call or microphone, callServiceTypes(34, microphone = true, camera = false))
        assertEquals(call, callServiceTypes(29, microphone = true, camera = true))
        assertEquals(0, callServiceTypes(28, microphone = true, camera = true))
    }

    // Like the microphone, the camera type starts only with the app on the screen or from the
    // user's own tap on the notification (the documented exception); never without the permission,
    // and only while the call has video.
    @Test
    fun theCameraTypeNeedsVideoThePermissionAndTheUser() {
        assertTrue(cameraServiceAllowed(video = true, granted = true, visible = true, fromNotification = false))
        assertTrue(cameraServiceAllowed(video = true, granted = true, visible = false, fromNotification = true))
        assertFalse(cameraServiceAllowed(video = true, granted = true, visible = false, fromNotification = false))
        assertFalse(cameraServiceAllowed(video = true, granted = false, visible = true, fromNotification = true))
        assertFalse(cameraServiceAllowed(video = false, granted = true, visible = true, fromNotification = true))
    }

    // The ongoing notification's camera action has its own tap: the core turns our camera on. It
    // is never taken for the incoming call notification's answer or decline.
    @Test
    fun theNotificationsCameraActionAsksForVideo() {
        assertEquals(CallEvent.VideoRequested, tapCallEvent(NotificationTap.CALL_VIDEO))
        assertEquals(NotificationTap.CALL_VIDEO, notificationTapOf("com.flickertalk.platform.CALL_VIDEO"))
        assertFalse(NotificationTap.CALL_VIDEO.action == NotificationTap.CALL_ANSWER.action)
    }

    // The bridge's manifest declares what the camera type needs, and the service may hold it.
    @Test
    fun theManifestDeclaresTheCameraForTheCallService() {
        val manifest = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(File("src/main/AndroidManifest.xml"))
        val permissions = manifest.getElementsByTagName("uses-permission").let { nodes ->
            (0 until nodes.length).map { (nodes.item(it) as Element).getAttribute("android:name") }
        }
        assertTrue(permissions.contains("android.permission.CAMERA"))
        assertTrue(permissions.contains("android.permission.FOREGROUND_SERVICE_CAMERA"))
        val services = manifest.getElementsByTagName("service")
        val call = (0 until services.length).map { services.item(it) as Element }
            .first { it.getAttribute("android:name") == "com.flickertalk.platform.FtCallService" }
        assertEquals(setOf("phoneCall", "microphone", "camera"), call.getAttribute("android:foregroundServiceType").split("|").toSet())
    }
}
