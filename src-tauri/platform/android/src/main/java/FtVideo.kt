package com.flickertalk.platform

import android.app.Activity
import android.content.Context
import android.graphics.Color
import android.hardware.display.DisplayManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.Display
import android.view.Gravity
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import android.widget.FrameLayout
import app.tauri.PermissionState
import app.tauri.annotation.InvokeArg
import kotlin.math.roundToInt

// ---- Native video (2026-09-29, docs/video-nativo.md) ----
// The call's pictures live in Rust (`ft-media`); here they get their place on the screen: two
// SurfaceViews behind the WebView, which turns transparent where the WebView leaves room for them.
// The WebView keeps every control (HTML, with its i18n and accessibility) and every touch.

/** A rectangle of the WebView in CSS pixels, from its top left corner (`getBoundingClientRect`). */
data class CssRect(val x: Double, val y: Double, val width: Double, val height: Double)

/** A rectangle in the screen's pixels, as a view is laid out in the WebView's parent. */
data class PixelRect(val left: Int, val top: Int, val width: Int, val height: Int)

/**
 * CSS pixels to the screen's: one CSS pixel is `density` pixels at the WebView's zoom of 1. Each
 * edge rounds on its own, so rects that touch leave no gap. `offsetX`/`offsetY`: where the WebView
 * starts in the parent the views share with it.
 */
fun cssToPixels(rect: CssRect, density: Float, offsetX: Int = 0, offsetY: Int = 0): PixelRect {
    val left = (rect.x * density).roundToInt()
    val top = (rect.y * density).roundToInt()
    val right = ((rect.x + rect.width) * density).roundToInt()
    val bottom = ((rect.y + rect.height) * density).roundToInt()
    return PixelRect(left + offsetX, top + offsetY, right - left, bottom - top)
}

/**
 * Where a hidden view waits: off the screen, one pixel big. Hiding a SurfaceView (`GONE`) would
 * destroy its surface; this keeps it, so the engine keeps its window.
 */
val PARKED = PixelRect(-1, -1, 1, 1)

/** Where a view goes: its rect, or parked when the WebView shows no room for it (`null`). */
fun viewPlacement(rect: CssRect?, density: Float, offsetX: Int = 0, offsetY: Int = 0): PixelRect =
    rect?.let { cssToPixels(it, density, offsetX, offsetY) } ?: PARKED

/**
 * The other side's picture, whole and centred in its room: a SurfaceView stretches what it shows,
 * so the view takes the picture's shape (`videoShape`, upright size). Unknown size: the room.
 */
fun fitPicture(room: PixelRect, width: Int, height: Int): PixelRect {
    if (width <= 0 || height <= 0 || room.width <= 0 || room.height <= 0) return room
    val scale = minOf(room.width.toDouble() / width, room.height.toDouble() / height)
    val fittedWidth = (width * scale).roundToInt()
    val fittedHeight = (height * scale).roundToInt()
    return PixelRect(room.left + (room.width - fittedWidth) / 2, room.top + (room.height - fittedHeight) / 2, fittedWidth, fittedHeight)
}

/** The display's rotation (`Surface.ROTATION_*`) in degrees, as the core takes it. */
fun rotationDegrees(rotation: Int): Int = when (rotation) {
    Surface.ROTATION_90 -> 90
    Surface.ROTATION_180 -> 180
    Surface.ROTATION_270 -> 270
    else -> 0
}

/** What `requestCamera` does with the permission's state. */
enum class CameraRequest { GRANTED, ASK, DENIED }

/**
 * Allowed answers at once; anything else asks the system, which answers at once by itself if the
 * user said "never"; a permission the build does not declare (`null`) is a no.
 */
fun cameraRequest(state: PermissionState?): CameraRequest = when (state) {
    PermissionState.GRANTED -> CameraRequest.GRANTED
    null -> CameraRequest.DENIED
    else -> CameraRequest.ASK
}

/**
 * Whether the call's service may add the camera type: while the call has video, with the camera
 * allowed, and only with the app on the screen or from the user's own tap on the notification
 * (Android's documented exception); otherwise the system refuses the whole service.
 */
fun cameraServiceAllowed(video: Boolean, granted: Boolean, visible: Boolean, fromNotification: Boolean): Boolean =
    video && granted && (visible || fromNotification)

/** The ongoing call notification's camera action, as `CALL_ACTION` carries it. */
const val VIDEO_ACTION = "video"

/** Whether the app was opened by the ongoing call notification's camera action. */
fun asksForVideo(value: String?): Boolean = value == VIDEO_ACTION

/**
 * The views' surfaces to Rust (`src-tauri/src/video_surfaces.rs`, JNI): `ANativeWindow`s for the
 * engine's display (remote) and camera preview (local). The app's library is loaded by Tauri.
 */
object FtVideoSurfaces {
    /** The other side's picture (`ft_media::ViewSlot::Remote`). */
    const val REMOTE = 0
    /** Our preview (`ft_media::ViewSlot::Local`). */
    const val LOCAL = 1

    @JvmStatic
    external fun nativeSurface(slot: Int, surface: Surface?)

    /** To Rust, if its library is there (it always is in the app; not in the JVM tests). */
    fun hand(slot: Int, surface: Surface?) {
        try {
            nativeSurface(slot, surface)
        } catch (_: UnsatisfiedLinkError) {
        }
    }
}

/** A view's surface, each time it is made or changes, and null before it is destroyed. */
private class SurfaceHandOff(private val slot: Int) : SurfaceHolder.Callback {
    override fun surfaceCreated(holder: SurfaceHolder) = FtVideoSurfaces.hand(slot, holder.surface)

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) =
        FtVideoSurfaces.hand(slot, holder.surface)

    // Rust must be done with the window before this returns: the call is synchronous.
    override fun surfaceDestroyed(holder: SurfaceHolder) = FtVideoSurfaces.hand(slot, null)
}

@InvokeArg
class VideoRectArgs {
    var x: Double = 0.0
    var y: Double = 0.0
    var width: Double = 0.0
    var height: Double = 0.0

    fun rect() = CssRect(x, y, width, height)
}

/** `videoLayout`: where the WebView leaves room for each picture; `null` hides it. */
@InvokeArg
class VideoLayoutArgs {
    var remote: VideoRectArgs? = null
    var local: VideoRectArgs? = null
    /** Our preview as in a mirror: the engine's preview draws it so; nothing to do here. */
    var mirrorLocal: Boolean = false
    /** Rounded corners: a SurfaceView does not clip, so our preview is square here. */
    var localRadius: Double = 0.0
}

/** `videoShape`: the other side's picture, upright (Android's decoder turns it itself). */
@InvokeArg
class VideoShapeArgs {
    var width: Int = 0
    var height: Int = 0
    var rotation: Int = 0
}

/** `callVideo`: whether the call has video now (either camera on). */
@InvokeArg
class CallVideoArgs {
    var on: Boolean = false
}

/**
 * The call's video views behind the WebView, from `attachVideo` to `detachVideo`. Used on the
 * main thread.
 */
object CallVideoViews {
    private var container: FrameLayout? = null
    private var remote: SurfaceView? = null
    private var local: SurfaceView? = null
    private var webView: WebView? = null
    private var remoteRoom: CssRect? = null
    private var localRoom: CssRect? = null
    private var pictureWidth = 0
    private var pictureHeight = 0
    private var displays: DisplayManager? = null
    private var lastRotation = -1
    private val turns = object : DisplayManager.DisplayListener {
        override fun onDisplayAdded(displayId: Int) {}

        override fun onDisplayRemoved(displayId: Int) {}

        override fun onDisplayChanged(displayId: Int) = tellRotation()
    }
    private val relayout = View.OnLayoutChangeListener { _, _, _, _, _, _, _, _, _ -> place() }

    val attached: Boolean
        get() = container != null

    /**
     * A FrameLayout at index 0 of the WebView's parent (wry's `setContentView(webView)`: the
     * activity's content), with the remote SurfaceView and ours over it (`setZOrderMediaOverlay`:
     * above the remote, still under the window). Both parked until `layout` places them. The
     * WebView turns transparent.
     */
    fun attach(activity: Activity, web: WebView) {
        if (container != null) return
        val parent = web.parent as? ViewGroup ?: return
        val box = FrameLayout(activity).apply { setBackgroundColor(Color.BLACK) }
        val remoteView = SurfaceView(activity).apply { holder.addCallback(SurfaceHandOff(FtVideoSurfaces.REMOTE)) }
        val localView = SurfaceView(activity).apply {
            setZOrderMediaOverlay(true)
            holder.addCallback(SurfaceHandOff(FtVideoSurfaces.LOCAL))
        }
        box.addView(remoteView, layoutParams(PARKED))
        box.addView(localView, layoutParams(PARKED))
        parent.addView(box, 0, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        web.setBackgroundColor(Color.TRANSPARENT)
        web.addOnLayoutChangeListener(relayout)
        container = box
        remote = remoteView
        local = localView
        webView = web
        displays = activity.getSystemService(DisplayManager::class.java)?.also {
            it.registerDisplayListener(turns, Handler(Looper.getMainLooper()))
        }
        lastRotation = -1
        tellRotation()
    }

    fun layout(args: VideoLayoutArgs) {
        remoteRoom = args.remote?.rect()
        localRoom = args.local?.rect()
        place()
    }

    fun shape(width: Int, height: Int) {
        pictureWidth = width
        pictureHeight = height
        place()
    }

    /** Takes the views away (their surfaces go to Rust as null) and the WebView is opaque again. */
    fun detach() {
        displays?.unregisterDisplayListener(turns)
        displays = null
        container?.let { (it.parent as? ViewGroup)?.removeView(it) }
        webView?.let {
            it.removeOnLayoutChangeListener(relayout)
            it.setBackgroundColor(Color.WHITE)
        }
        container = null
        remote = null
        local = null
        webView = null
        remoteRoom = null
        localRoom = null
        pictureWidth = 0
        pictureHeight = 0
    }

    private fun place() {
        val web = webView ?: return
        val box = container ?: return
        val density = web.resources.displayMetrics.density
        val offsetX = web.left - box.left
        val offsetY = web.top - box.top
        val room = remoteRoom?.let { cssToPixels(it, density, offsetX, offsetY) }
        move(remote, room?.let { fitPicture(it, pictureWidth, pictureHeight) } ?: PARKED)
        move(local, viewPlacement(localRoom, density, offsetX, offsetY))
    }

    private fun move(view: SurfaceView?, rect: PixelRect) {
        view ?: return
        val params = view.layoutParams as? FrameLayout.LayoutParams ?: return
        if (params.leftMargin == rect.left && params.topMargin == rect.top && params.width == rect.width && params.height == rect.height) return
        params.leftMargin = rect.left
        params.topMargin = rect.top
        params.width = rect.width
        params.height = rect.height
        view.layoutParams = params
    }

    private fun layoutParams(rect: PixelRect) = FrameLayout.LayoutParams(rect.width, rect.height).apply {
        gravity = Gravity.TOP or Gravity.START
        leftMargin = rect.left
        topMargin = rect.top
    }

    /** The display's rotation to the core, when it changes, for the rotation our frames carry. */
    private fun tellRotation() {
        val web = webView ?: return
        val display = displayOf(web) ?: return
        val degrees = rotationDegrees(display.rotation)
        if (degrees == lastRotation) return
        lastRotation = degrees
        CallEvents.offer(CallEvent.Orientation(degrees))
    }

    private fun displayOf(web: WebView): Display? {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) web.display?.let { return it }
        @Suppress("DEPRECATION")
        return (web.context.getSystemService(Context.WINDOW_SERVICE) as? android.view.WindowManager)?.defaultDisplay
    }
}
