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

class FtVideoTest {
    @Test
    fun cssPixelsBecomeScreenPixels() {
        assertEquals(PixelRect(0, 0, 1080, 2340), cssToPixels(CssRect(0.0, 0.0, 360.0, 780.0), 3f))
        assertEquals(PixelRect(834, 1782, 288, 420), cssToPixels(CssRect(278.0, 594.0, 96.0, 140.0), 3f))
    }

    @Test
    fun fractionsRoundByTheEdges() {
        val left = cssToPixels(CssRect(0.0, 0.0, 100.3, 50.0), 2.625f)
        val right = cssToPixels(CssRect(100.3, 0.0, 100.3, 50.0), 2.625f)
        assertEquals(left.left + left.width, right.left)
        assertEquals(263, left.width)
    }

    @Test
    fun theWebViewsOwnPlaceMovesTheViews() {
        assertEquals(PixelRect(10, 110, 300, 300), cssToPixels(CssRect(0.0, 0.0, 100.0, 100.0), 3f, offsetX = 10, offsetY = 110))
    }

    @Test
    fun aHiddenViewWaitsOffTheScreen() {
        val parked = viewPlacement(null, 3f)
        assertTrue(parked.left + parked.width <= 0 && parked.top + parked.height <= 0)
        assertTrue(parked.width > 0 && parked.height > 0)
        assertEquals(PixelRect(30, 60, 90, 120), viewPlacement(CssRect(10.0, 20.0, 30.0, 40.0), 3f))
    }

    @Test
    fun theRemotePictureKeepsItsShape() {
        val room = PixelRect(0, 0, 1080, 2340)
        assertEquals(PixelRect(0, 765, 1080, 810), fitPicture(room, 640, 480))
        assertEquals(PixelRect(0, 210, 1080, 1920), fitPicture(room, 720, 1280))
        assertEquals(PixelRect(40, 0, 1000, 500), fitPicture(PixelRect(0, 0, 1080, 500), 1280, 640))
    }

    @Test
    fun aPictureOfUnknownSizeFillsItsRoom() {
        val room = PixelRect(5, 6, 700, 900)
        assertEquals(room, fitPicture(room, 0, 0))
        assertEquals(room, fitPicture(room, 640, 0))
    }

    @Test
    fun theSurfaceSlotsMatchRust() {
        assertEquals(0, FtVideoSurfaces.REMOTE)
        assertEquals(1, FtVideoSurfaces.LOCAL)
    }

    @Test
    fun theDisplayRotationIsInDegrees() {
        assertEquals(0, rotationDegrees(Surface.ROTATION_0))
        assertEquals(90, rotationDegrees(Surface.ROTATION_90))
        assertEquals(180, rotationDegrees(Surface.ROTATION_180))
        assertEquals(270, rotationDegrees(Surface.ROTATION_270))
    }

    @Test
    fun videoEventsTravelAsTheCoreReadsThem() {
        assertEquals(mapOf("event" to "visible", "visible" to true), callEventPayload(CallEvent.Visible(true)))
        assertEquals(mapOf("event" to "visible", "visible" to false), callEventPayload(CallEvent.Visible(false)))
        assertEquals(mapOf("event" to "orientation", "orientation" to 270), callEventPayload(CallEvent.Orientation(270)))
        assertEquals(mapOf("event" to "video"), callEventPayload(CallEvent.VideoRequested))
    }

    @Test
    fun theCameraPermissionIsAskedOnlyWhenItIsNotAllowed() {
        assertEquals(CameraRequest.GRANTED, cameraRequest(PermissionState.GRANTED))
        assertEquals(CameraRequest.ASK, cameraRequest(PermissionState.PROMPT))
        assertEquals(CameraRequest.ASK, cameraRequest(PermissionState.PROMPT_WITH_RATIONALE))
        assertEquals(CameraRequest.ASK, cameraRequest(PermissionState.DENIED))
        assertEquals(CameraRequest.DENIED, cameraRequest(null))
    }

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

    @Test
    fun theCameraTypeNeedsVideoThePermissionAndTheUser() {
        assertTrue(cameraServiceAllowed(video = true, granted = true, visible = true, fromNotification = false))
        assertTrue(cameraServiceAllowed(video = true, granted = true, visible = false, fromNotification = true))
        assertFalse(cameraServiceAllowed(video = true, granted = true, visible = false, fromNotification = false))
        assertFalse(cameraServiceAllowed(video = true, granted = false, visible = true, fromNotification = true))
        assertFalse(cameraServiceAllowed(video = false, granted = true, visible = true, fromNotification = true))
    }

    @Test
    fun theNotificationsCameraActionAsksForVideo() {
        assertEquals(CallEvent.VideoRequested, tapCallEvent(NotificationTap.CALL_VIDEO))
        assertEquals(NotificationTap.CALL_VIDEO, notificationTapOf("com.flickertalk.platform.CALL_VIDEO"))
        assertFalse(NotificationTap.CALL_VIDEO.action == NotificationTap.CALL_ANSWER.action)
    }

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
