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

data class CssRect(val x: Double, val y: Double, val width: Double, val height: Double)

data class PixelRect(val left: Int, val top: Int, val width: Int, val height: Int)

// round each edge, not the size, so rects that touch leave no gap
fun cssToPixels(rect: CssRect, density: Float, offsetX: Int = 0, offsetY: Int = 0): PixelRect {
    val left = (rect.x * density).roundToInt()
    val top = (rect.y * density).roundToInt()
    val right = ((rect.x + rect.width) * density).roundToInt()
    val bottom = ((rect.y + rect.height) * density).roundToInt()
    return PixelRect(left + offsetX, top + offsetY, right - left, bottom - top)
}

// GONE would destroy the surface and the engine's window with it, so park it off screen
val PARKED = PixelRect(-1, -1, 1, 1)

fun viewPlacement(rect: CssRect?, density: Float, offsetX: Int = 0, offsetY: Int = 0): PixelRect =
    rect?.let { cssToPixels(it, density, offsetX, offsetY) } ?: PARKED

// SurfaceView stretches what it shows, so the view takes the picture's shape
fun fitPicture(room: PixelRect, width: Int, height: Int): PixelRect {
    if (width <= 0 || height <= 0 || room.width <= 0 || room.height <= 0) return room
    val scale = minOf(room.width.toDouble() / width, room.height.toDouble() / height)
    val fittedWidth = (width * scale).roundToInt()
    val fittedHeight = (height * scale).roundToInt()
    return PixelRect(room.left + (room.width - fittedWidth) / 2, room.top + (room.height - fittedHeight) / 2, fittedWidth, fittedHeight)
}

fun rotationDegrees(rotation: Int): Int = when (rotation) {
    Surface.ROTATION_90 -> 90
    Surface.ROTATION_180 -> 180
    Surface.ROTATION_270 -> 270
    else -> 0
}

enum class CameraRequest { GRANTED, ASK, DENIED }

fun cameraRequest(state: PermissionState?): CameraRequest = when (state) {
    PermissionState.GRANTED -> CameraRequest.GRANTED
    null -> CameraRequest.DENIED
    else -> CameraRequest.ASK
}

// Android refuses the whole service if the camera type is added off screen, unless from a tap on our notification
fun cameraServiceAllowed(video: Boolean, granted: Boolean, visible: Boolean, fromNotification: Boolean): Boolean =
    video && granted && (visible || fromNotification)

// JNI, implemented in src-tauri/src/video_surfaces.rs
object FtVideoSurfaces {
    const val REMOTE = 0
    const val LOCAL = 1

    @JvmStatic
    external fun nativeSurface(slot: Int, surface: Surface?)

    // no native library in the JVM tests
    fun hand(slot: Int, surface: Surface?) {
        try {
            nativeSurface(slot, surface)
        } catch (_: UnsatisfiedLinkError) {
        }
    }
}

private class SurfaceHandOff(private val slot: Int) : SurfaceHolder.Callback {
    override fun surfaceCreated(holder: SurfaceHolder) = FtVideoSurfaces.hand(slot, holder.surface)

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) =
        FtVideoSurfaces.hand(slot, holder.surface)

    // Rust has to be done with the window before this returns
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

@InvokeArg
class VideoLayoutArgs {
    var remote: VideoRectArgs? = null
    var local: VideoRectArgs? = null
    var mirrorLocal: Boolean = false
    // SurfaceView can't clip, so our preview stays square
    var localRadius: Double = 0.0
}

// Android's decoder already turns the picture upright
@InvokeArg
class VideoShapeArgs {
    var width: Int = 0
    var height: Int = 0
    var rotation: Int = 0
}

@InvokeArg
class CallVideoArgs {
    var on: Boolean = false
}

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

    // setZOrderMediaOverlay keeps our preview above the remote view, still under the window
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
