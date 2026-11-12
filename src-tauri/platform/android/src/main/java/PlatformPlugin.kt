package com.flickertalk.platform

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.res.Configuration
import android.os.Build
import android.util.Base64
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import app.tauri.PermissionState
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import kotlin.math.roundToInt

internal const val PREFERENCES = "ft.platform"

// the WebView reads the font scale only when it's created, and the activity isn't recreated any more
fun textZoomFor(fontScale: Float): Int = (fontScale * 100).roundToInt()

@InvokeArg
class CallEventsArgs {
    lateinit var channel: Channel
}

@TauriPlugin(
    permissions = [
        Permission(strings = [Manifest.permission.RECORD_AUDIO], alias = "microphone"),
        Permission(strings = [Manifest.permission.CAMERA], alias = "camera"),
        // both, so the user can pick precise or approximate
        Permission(strings = [Manifest.permission.ACCESS_COARSE_LOCATION, Manifest.permission.ACCESS_FINE_LOCATION], alias = "location"),
    ],
)
class PlatformPlugin(private val activity: Activity) : Plugin(activity) {
    private val authorization = Authorization(activity)

    private var webView: WebView? = null

    private val ringing = CoreRinging(activity)

    private val files = Files(activity)

    private val billing = Billing(activity)

    override fun load(webView: WebView) {
        super.load(webView)
        this.webView = webView
        // wry has set its WebViewClient by now, so it can be wrapped
        activity.runOnUiThread { serveAssetRanges(webView, activity.filesDir) }
        InCall.appVisible = true
        cancelActivityNotification(activity)
        if (IncomingCall.showing) showOverLockScreen(activity, overLockScreen(ringing = true, inCall = InCall.active))
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        webView?.settings?.textZoom = textZoomFor(newConfig.fontScale)
    }

    override fun onResume() {
        super.onResume()
        InCall.appVisible = true
        CallEvents.offer(CallEvent.Visible(true))
        if (InCall.active) InCall.service?.promote()
        billing.tellEntitlement()
    }

    override fun onPause() {
        super.onPause()
        InCall.appVisible = false
        CallEvents.offer(CallEvent.Visible(false))
        ringing.appHidden()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        authorization.redirected(intent)
    }

    @Command
    fun pendingReminder(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("reminder", tappedReminder.take()) })
    }

    @Command
    fun openedLink(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("url", openedLinks.take()) })
    }

    @Command
    fun registerLinkEvents(invoke: Invoke) {
        try {
            val channel = invoke.parseArgs(CallEventsArgs::class.java).channel
            openedLinks.register { channel.sendObject(mapOf("event" to "linkOpened")) }
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "no channel")
        }
    }

    @Command
    fun setReminders(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(RemindersArgs::class.java)
            scheduleReminders(activity, parseReminders(args.reminders))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot set the reminders")
        }
    }

    @Command
    fun exactAlarms(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("allowed", exactAlarmsAllowed(activity)) })
    }

    @Command
    fun requestExactAlarms(invoke: Invoke) {
        val allowed = exactAlarmsAllowed(activity)
        try {
            askForExactAlarms(activity, allowed)
            invoke.resolve(JSObject().apply { put("allowed", allowed) })
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot open the setting")
        }
    }

    @Command
    fun authorize(invoke: Invoke) = authorization.authorize(invoke)

    @Command
    fun callAnswering(invoke: Invoke) {
        ringing.answering(invoke.parseArgs(AnsweringArgs::class.java))
        invoke.resolve()
    }

    @Command
    fun registerCallEvents(invoke: Invoke) {
        try {
            CallEvents.register(invoke.parseArgs(CallEventsArgs::class.java).channel)
            CallEvents.offer(CallEvent.Visible(InCall.appVisible))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "no channel")
        }
    }

    @Command
    fun callStartedOutgoing(invoke: Invoke) {
        val args = invoke.parseArgs(OutgoingArgs::class.java)
        activity.runOnUiThread { InCall.start(activity, args.name, args.video) }
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = true))
        keepScreenOn(activity, inCall = true)
        invoke.resolve()
    }

    @Command
    fun callConnected(invoke: Invoke) {
        activity.runOnUiThread { InCall.start(activity, InCall.ringingName) }
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = true))
        keepScreenOn(activity, inCall = true)
        invoke.resolve()
    }

    @Command
    fun callEnded(invoke: Invoke) {
        activity.runOnUiThread { InCall.end(activity) }
        showOverLockScreen(activity, overLockScreen(ringing = false, inCall = false))
        keepScreenOn(activity, inCall = false)
        invoke.resolve()
    }

    @Command
    fun setSystemBars(invoke: Invoke) {
        val args = invoke.parseArgs(SystemBarsArgs::class.java)
        val look = systemBarsLook(args.dark, Build.VERSION.SDK_INT)
        activity.runOnUiThread {
            showSystemBars(activity.window, look)
            invoke.resolve()
        }
    }

    @Command
    fun setSpeaker(invoke: Invoke) {
        val args = invoke.parseArgs(SpeakerArgs::class.java)
        activity.runOnUiThread { InCall.setSpeaker(activity, args.on) }
        invoke.resolve()
    }

    @Command
    fun requestMicrophone(invoke: Invoke) {
        if (getPermissionState("microphone") == PermissionState.GRANTED) {
            invoke.resolve(JSObject().apply { put("granted", true) })
        } else {
            requestPermissionForAlias("microphone", invoke, "microphoneAnswered")
        }
    }

    @PermissionCallback
    fun microphoneAnswered(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("granted", getPermissionState("microphone") == PermissionState.GRANTED) })
    }

    @Command
    fun currentLocation(invoke: Invoke) {
        val allowed = holds(activity, Manifest.permission.ACCESS_FINE_LOCATION) || holds(activity, Manifest.permission.ACCESS_COARSE_LOCATION)
        if (allowed) {
            locate(invoke)
        } else {
            requestPermissionForAlias("location", invoke, "locationAnswered")
        }
    }

    @PermissionCallback
    fun locationAnswered(invoke: Invoke) {
        locate(invoke)
    }

    private fun locate(invoke: Invoke) {
        activity.runOnUiThread {
            currentFix(activity) { fix ->
                invoke.resolve(JSObject().apply { locationPayload(fix).forEach { (key, value) -> put(key, value) } })
            }
        }
    }

    // rejections carry the error kind, never the text
    @Command
    fun translateLanguages(invoke: Invoke) = translating(invoke) {
        invoke.resolve(JSObject().apply { put("languages", JSArray(supportedTags())) })
    }

    @Command
    fun translateDetect(invoke: Invoke) = translating(invoke) {
        val args = invoke.parseArgs(TranslateSourceArgs::class.java)
        detectLanguage(args.text) { tag -> invoke.resolve(jsObject(detectPayload(tag))) }
    }

    @Command
    fun translateStatus(invoke: Invoke) = translating(invoke) {
        val args = invoke.parseArgs(TranslatePairArgs::class.java)
        pairStatus(args.from, args.to) { status ->
            if (status == null) invoke.reject("failed: models") else invoke.resolve(JSObject().apply { put("status", status) })
        }
    }

    @Command
    fun translatePrepare(invoke: Invoke) = translating(invoke) {
        val args = invoke.parseArgs(TranslatePairArgs::class.java)
        preparePair(activity, args.from, args.to) { ready -> invoke.resolve(JSObject().apply { put("ready", ready) }) }
    }

    @Command
    fun translateRemove(invoke: Invoke) = translating(invoke) {
        val args = invoke.parseArgs(TranslatePairArgs::class.java)
        removePair(args.from, args.to) { removed -> invoke.resolve(JSObject().apply { put("removed", removed) }) }
    }

    @Command
    fun translateText(invoke: Invoke) = translating(invoke) {
        val args = invoke.parseArgs(TranslateTextArgs::class.java)
        translateOnPhone(args.text, args.from, args.to) { text -> invoke.resolve(jsObject(textPayload(text))) }
    }

    private fun translating(invoke: Invoke, work: () -> Unit) {
        try {
            work()
        } catch (error: Exception) {
            invoke.reject("failed: ${error.javaClass.simpleName}")
        }
    }

    private fun jsObject(payload: Map<String, Any>) = JSObject().apply { payload.forEach { (key, value) -> put(key, value) } }

    @Command
    fun requestCamera(invoke: Invoke) {
        when (cameraRequest(getPermissionState("camera"))) {
            CameraRequest.GRANTED -> invoke.resolve(JSObject().apply { put("granted", true) })
            CameraRequest.DENIED -> invoke.resolve(JSObject().apply { put("granted", false) })
            CameraRequest.ASK -> requestPermissionForAlias("camera", invoke, "cameraAnswered")
        }
    }

    @PermissionCallback
    fun cameraAnswered(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("granted", getPermissionState("camera") == PermissionState.GRANTED) })
    }

    @Command
    fun attachVideo(invoke: Invoke) {
        val web = webView
        if (web == null) {
            invoke.reject("no webview")
            return
        }
        activity.runOnUiThread {
            CallVideoViews.attach(activity, web)
            invoke.resolve()
        }
    }

    @Command
    fun videoLayout(invoke: Invoke) {
        val args = invoke.parseArgs(VideoLayoutArgs::class.java)
        activity.runOnUiThread {
            CallVideoViews.layout(args)
            invoke.resolve()
        }
    }

    @Command
    fun videoShape(invoke: Invoke) {
        val args = invoke.parseArgs(VideoShapeArgs::class.java)
        activity.runOnUiThread {
            CallVideoViews.shape(args.width, args.height)
            invoke.resolve()
        }
    }

    @Command
    fun detachVideo(invoke: Invoke) {
        activity.runOnUiThread {
            CallVideoViews.detach()
            invoke.resolve()
        }
    }

    @Command
    fun callVideo(invoke: Invoke) {
        val on = invoke.parseArgs(CallVideoArgs::class.java).on
        activity.runOnUiThread {
            switchCallVideo(activity, on)
            invoke.resolve()
        }
    }

    @Command
    fun canShowFullScreen(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("allowed", fullScreenAllowed(activity)) })
    }

    @Command
    fun askFullScreen(invoke: Invoke) {
        try {
            askForFullScreen(activity)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot open the setting")
        }
    }

    @Command
    fun startRinging(invoke: Invoke) {
        try {
            ringing.start(invoke.parseArgs(RingingArgs::class.java))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot ring")
        }
    }

    @Command
    fun sealKey(invoke: Invoke) {
        try {
            val key = Base64.decode(invoke.parseArgs(KeyArgs::class.java).value, Base64.NO_WRAP)
            val sealed = sealStorageKey(key)
            invoke.resolve(JSObject().apply { put("value", Base64.encodeToString(sealed, Base64.NO_WRAP)) })
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot seal the key")
        }
    }

    @Command
    fun forgetKey(invoke: Invoke) {
        try {
            forgetStorageKey()
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot forget the key")
        }
    }

    @Command
    fun openKey(invoke: Invoke) {
        try {
            val sealed = Base64.decode(invoke.parseArgs(KeyArgs::class.java).value, Base64.NO_WRAP)
            invoke.resolve(JSObject().apply { put("value", Base64.encodeToString(openStorageKey(sealed), Base64.NO_WRAP)) })
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot open the key")
        }
    }

    @Command
    fun pushToken(invoke: Invoke) = sendPushToken(invoke)

    @Command
    fun requestNotifications(invoke: Invoke) {
        askForNotifications(activity)
        invoke.resolve()
    }

    @Command
    fun restartApp(invoke: Invoke) {
        val launch = activity.packageManager.getLaunchIntentForPackage(activity.packageName)
        if (launch?.component == null) {
            invoke.reject("no launcher activity")
            return
        }
        invoke.resolve()
        activity.startActivity(Intent.makeRestartActivityTask(launch.component))
        Runtime.getRuntime().exit(0)
    }

    @Command
    fun setOpenSlots(invoke: Invoke) {
        keepOpenSlots(activity, invoke.parseArgs(OpenSlotsArgs::class.java).slots.toSet())
        invoke.resolve()
    }

    @Command
    fun setQuietHours(invoke: Invoke) {
        keepQuietHours(activity, invoke.parseArgs(QuietHoursArgs::class.java).week)
        invoke.resolve()
    }

    @Command
    fun callRefused(invoke: Invoke) {
        ringing.refused()
        invoke.resolve()
    }

    @Command
    fun stopRinging(invoke: Invoke) {
        ringing.stop()
        invoke.resolve()
    }

    @Command
    fun shareText(invoke: Invoke) {
        try {
            val text = shareableText(invoke.parseArgs(ShareTextArgs::class.java).text)
            if (text == null) {
                invoke.reject("nothing to share")
                return
            }
            files.shareText(text)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot share")
        }
    }

    // the WebView's file input can't be left without picking something, the system picker can
    @Command
    fun pickFiles(invoke: Invoke) {
        val accept = try {
            invoke.parseArgs(PickArgs::class.java).accept
        } catch (_: Exception) {
            ""
        }
        startActivityForResult(invoke, pickIntent(accept), "picked")
    }

    @Command
    fun takePhoto(invoke: Invoke) {
        val intent = files.photoIntent()
        if (intent == null) {
            invoke.resolve(JSObject().apply { put("files", JSArray()) })
            return
        }
        startActivityForResult(invoke, intent, "photoTaken")
    }

    @ActivityCallback
    fun photoTaken(invoke: Invoke, result: ActivityResult) {
        invoke.resolve(JSObject().apply { put("files", files.photoTaken(result.resultCode)) })
    }

    @ActivityCallback
    fun picked(invoke: Invoke, result: ActivityResult) {
        val picked = try {
            files.copyPicked(result.data)
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot read what was picked")
            return
        }
        invoke.resolve(JSObject().apply { put("files", picked) })
    }

    @Command
    fun openFile(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(OpenFileArgs::class.java)
            files.openFile(args.path, args.mime)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot open the file")
        }
    }

    @Command
    fun shareFile(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SaveFileArgs::class.java)
            files.shareFile(args.path, args.mime)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot share the file")
        }
    }

    @Command
    fun subscriptionPrice(invoke: Invoke) = billing.subscriptionPrice(invoke)

    @Command
    fun subscribe(invoke: Invoke) = billing.subscribe(invoke)

    @Command
    fun subscription(invoke: Invoke) = billing.subscription(invoke)

    @Command
    fun registerEntitlementEvents(invoke: Invoke) {
        try {
            EntitlementEvents.register(invoke.parseArgs(CallEventsArgs::class.java).channel)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "no channel")
        }
    }

    @Command
    fun restoreSubscription(invoke: Invoke) = subscription(invoke)

    @Command
    fun saveToDownloads(invoke: Invoke) {
        if (!canSaveToDownloads(Build.VERSION.SDK_INT)) {
            invoke.reject("saving to Downloads needs Android 10")
            return
        }
        try {
            val args = invoke.parseArgs(SaveFileArgs::class.java)
            files.saveToDownloads(args.path, args.name, args.mime)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot save the file")
        }
    }

    // the file has to stay until the print service has read it
    @Command
    fun printFile(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SaveFileArgs::class.java)
            files.printFile(args.path, args.name, args.mime)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot print the file")
        }
    }
}
