package com.flickertalk.platform

import android.app.Activity
import android.app.ActivityManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.Ringtone
import android.media.RingtoneManager
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.graphics.BitmapFactory
import android.os.Bundle
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintDocumentInfo
import android.print.PrintManager
import android.provider.MediaStore
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import android.webkit.WebView
import androidx.core.app.NotificationCompat
import androidx.core.app.Person
import androidx.core.content.ContextCompat
import androidx.print.PrintHelper
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import androidx.core.content.FileProvider
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

/**
 * The plugin's own FileProvider class: the app already declares `androidx.core.content.FileProvider`
 * (wry's file chooser), and two providers of one class cannot be merged into the manifest.
 */
class FtFileProvider : FileProvider()

/** The most pictures the photo picker takes at once. */
const val PICK_LIMIT = 20

/** The FileProvider declared in this plugin's manifest. */
fun fileProviderAuthority(packageName: String): String = "$packageName.ft.files"

fun canSaveToDownloads(sdk: Int): Boolean = sdk >= Build.VERSION_CODES.Q

/**
 * Whether a request for pictures can use the photo picker: a sheet that opens over the app, closes
 * with a swipe and asks for no permission. Android 13 and up (§62).
 */
fun usesPhotoPicker(accept: String, sdk: Int): Boolean =
    accept.startsWith("image/") && sdk >= Build.VERSION_CODES.TIRAMISU

/** What the system's document picker is told to show. Anything we do not understand means all. */
fun documentType(accept: String): String =
    if (Regex("^[a-z]+/([a-z0-9.+-]+|\\*)$").matches(accept)) accept else "*/*"

/** The router's push only ever says "wake" (Plan §12): no sender, no content. */
fun isWake(data: Map<String, String>): Boolean = data["t"] == "wake"

/** An app on screen is already connected and gets everything: no notification then. */
fun shouldNotify(importance: Int): Boolean =
    importance > ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND

private const val CHANNEL = "ft.activity"
private const val NOTIFICATION = 1

/** Calls ring on their own channel, so messages and calls can be set apart (§66). */
const val CALL_CHANNEL = "ft.call"
private const val CALL_NOTIFICATION = 2

/** The notification's buttons travel as this extra on the intent that opens the app. */
const val CALL_ACTION = "ft.call.action"

/** What the user pressed on the call notification, if it was one of ours. */
fun callAction(value: String?): String = if (value == "answer" || value == "decline") value else ""

/** Who is calling; a contact with no name is still a caller. */
fun callTitle(name: String): String = name.trim().ifEmpty { "Someone" }

fun callText(video: Boolean): String = if (video) "Incoming video call" else "Incoming call"

/**
 * FCM wake-ups (M4). The push carries nothing to read: when the app is not on screen, a plain
 * notification invites the user to open it; opening it connects and fetches what waits.
 */
class FtMessagingService : FirebaseMessagingService() {
    override fun onMessageReceived(message: RemoteMessage) {
        if (!isWake(message.data)) return
        val state = ActivityManager.RunningAppProcessInfo()
        ActivityManager.getMyMemoryState(state)
        if (shouldNotify(state.importance)) showActivityNotification(this)
    }

    // A new token reaches the router the next time the app starts.
    override fun onNewToken(token: String) {}
}

/**
 * The incoming call on the screen: its own high channel, the call category and a full-screen
 * intent that opens the app over the lock screen. If the system does not allow full screen (§66,
 * Android 14 keeps it for calling apps), it still shows as a heads-up notification.
 */
/** Opening the app, carrying what the user pressed on the notification. */
private fun callIntent(context: Context, action: String, request: Int): PendingIntent? {
    val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        if (action.isNotEmpty()) putExtra(CALL_ACTION, action)
    } ?: return null
    return PendingIntent.getActivity(
        context,
        request,
        launch,
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
}

private fun showCall(context: Context, title: String, text: String) {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        manager.createNotificationChannel(
            NotificationChannel(CALL_CHANNEL, "Calls", NotificationManager.IMPORTANCE_HIGH).apply {
                setSound(null, null) // the ringtone is ours, so the notification stays quiet
                enableVibration(false)
            }
        )
    }
    val open = callIntent(context, "", 1) ?: return
    val answer = callIntent(context, "answer", 2) ?: return
    val decline = callIntent(context, "decline", 3) ?: return
    val caller = Person.Builder().setName(title).setImportant(true).build()
    val notification = NotificationCompat.Builder(context, CALL_CHANNEL)
        .setSmallIcon(R.drawable.ft_notification)
        .setContentTitle(title)
        .setContentText(text)
        .setPriority(NotificationCompat.PRIORITY_MAX)
        .setCategory(NotificationCompat.CATEGORY_CALL)
        .setOngoing(true)
        .setAutoCancel(true)
        .setContentIntent(open)
        .setFullScreenIntent(open, true)
        // Answer and decline from the notification itself: the user should not have to open the
        // app to pick up (§66).
        .setStyle(NotificationCompat.CallStyle.forIncomingCall(caller, decline, answer))
        .build()
    try {
        manager.notify(CALL_NOTIFICATION, notification)
    } catch (_: SecurityException) {
        // Notifications not allowed: the ringtone still plays.
    }
}

private fun showActivityNotification(context: Context) {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Messages and calls", NotificationManager.IMPORTANCE_HIGH)
        )
    }
    val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
    } ?: return
    val open = PendingIntent.getActivity(context, 0, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    val notification = NotificationCompat.Builder(context, CHANNEL)
        .setSmallIcon(R.drawable.ft_notification)
        .setContentTitle("FlickerTalk")
        .setContentText("Something new is waiting for you")
        .setPriority(NotificationCompat.PRIORITY_HIGH)
        .setCategory(NotificationCompat.CATEGORY_MESSAGE)
        .setAutoCancel(true)
        .setContentIntent(open)
        .build()
    try {
        manager.notify(NOTIFICATION, notification)
    } catch (_: SecurityException) {
        // Notifications not allowed: the app gets everything when it opens.
    }
}

private const val KEY_ALIAS = "ft.storage"
private const val IV_BYTES = 12

/** A sealed key is the GCM nonce followed by the ciphertext and its tag. */
fun splitSealed(sealed: ByteArray): Pair<ByteArray, ByteArray>? =
    if (sealed.size <= IV_BYTES) null else Pair(sealed.copyOfRange(0, IV_BYTES), sealed.copyOfRange(IV_BYTES, sealed.size))

/** The Keystore's AES key for the storage key: made once, never leaves the Keystore (§94). */
private fun keystoreKey(): SecretKey {
    val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    (store.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    generator.init(
        KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .build()
    )
    return generator.generateKey()
}

@InvokeArg
class KeyArgs {
    lateinit var value: String
}

/** How an incoming call rings. */
data class Ringing(val sound: Boolean, val vibrate: Boolean)

/** As the user set the phone: sound and vibration, vibration only, or nothing. */
fun ringingFor(ringerMode: Int): Ringing = when (ringerMode) {
    AudioManager.RINGER_MODE_NORMAL -> Ringing(sound = true, vibrate = true)
    AudioManager.RINGER_MODE_VIBRATE -> Ringing(sound = false, vibrate = true)
    else -> Ringing(sound = false, vibrate = false)
}

/** Ring 0.8 s, pause 1.2 s, and again. */
private val RING_PATTERN = longArrayOf(0, 800, 1200)

@InvokeArg
class RingingArgs {
    var caller: String = ""
    var video: Boolean = false
}

/** Where a picked file is copied, inside the app's own folder. */
const val PICKED_FOLDER = "uploads"

/** The name to show for a picked file; something with no name is still a file. */
fun pickedName(name: String?): String = name?.trim()?.takeIf { it.isNotEmpty() } ?: "file"

/** What a picked file is, as far as we can tell. */
fun pickedMime(mime: String?): String = mime?.trim()?.takeIf { it.isNotEmpty() } ?: "application/octet-stream"

/** The text for the share sheet, or null when there is nothing to share. */
fun shareableText(text: String): String? = text.trim().ifEmpty { null }

@InvokeArg
class ShareTextArgs {
    lateinit var text: String
}

/** What kind of file the app is asking the user for; empty means anything. */
@InvokeArg
class PickArgs {
    var accept: String = ""
}

@InvokeArg
class OpenFileArgs {
    lateinit var path: String
    lateinit var mime: String
}

@InvokeArg
class SaveFileArgs {
    lateinit var path: String
    lateinit var name: String
    lateinit var mime: String
}

/** What only Android lets Kotlin do (Plan §5): lend a file to a viewer, save it to Downloads. */
@TauriPlugin
class PlatformPlugin(private val activity: Activity) : Plugin(activity) {
    private var ringtone: Ringtone? = null
    private var vibrator: Vibrator? = null

    /** What the user pressed on the call notification, until the app asks for it. */
    private var pendingCall: String = ""

    /** The app is open: the "something new" notification has done its job. */
    override fun load(webView: WebView) {
        super.load(webView)
        activity.getSystemService(NotificationManager::class.java)?.cancel(NOTIFICATION)
        pendingCall = callAction(activity.intent?.getStringExtra(CALL_ACTION))
        activity.intent?.removeExtra(CALL_ACTION)
    }

    /** The app was already open when the notification's button was pressed. */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        val action = callAction(intent.getStringExtra(CALL_ACTION))
        if (action.isNotEmpty()) pendingCall = action
        intent.removeExtra(CALL_ACTION)
    }

    /** Answer or decline, once, if the user pressed it on the notification. */
    @Command
    fun pendingCall(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("action", pendingCall) })
        pendingCall = ""
    }

    /** Whether this phone lets us put a call on the whole screen (Android 14 and up). */
    @Command
    fun canShowFullScreen(invoke: Invoke) {
        val manager = activity.getSystemService(NotificationManager::class.java)
        val allowed = Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE ||
            manager?.canUseFullScreenIntent() == true
        invoke.resolve(JSObject().apply { put("allowed", allowed) })
    }

    /** Opens the system screen where the user allows calls to take the whole screen. */
    @Command
    fun askFullScreen(invoke: Invoke) {
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                activity.startActivity(
                    Intent(
                        android.provider.Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT,
                        android.net.Uri.parse("package:" + activity.packageName),
                    )
                )
            }
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot open the setting")
        }
    }

    /**
     * An incoming call (§66): the user's ringtone and vibration, and a call notification with a
     * full-screen intent, so the call shows on the screen even with the app in the background or
     * the phone locked. It all goes away with `stopRinging`.
     */
    @Command
    fun startRinging(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(RingingArgs::class.java)
            silence()
            showCall(activity, callTitle(args.caller), callText(args.video))
            val audio = activity.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            val ringing = ringingFor(audio.ringerMode)
            if (ringing.sound) {
                val uri = RingtoneManager.getActualDefaultRingtoneUri(activity, RingtoneManager.TYPE_RINGTONE)
                    ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE)
                ringtone = RingtoneManager.getRingtone(activity, uri)?.apply {
                    audioAttributes = AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build()
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) isLooping = true
                    play()
                }
            }
            if (ringing.vibrate && Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                vibrator = phoneVibrator()?.apply { vibrate(VibrationEffect.createWaveform(RING_PATTERN, 0)) }
            }
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot ring")
        }
    }

    /** Seals the storage key with the Keystore's AES key (§94). */
    @Command
    fun sealKey(invoke: Invoke) {
        try {
            val key = Base64.decode(invoke.parseArgs(KeyArgs::class.java).value, Base64.NO_WRAP)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, keystoreKey()) }
            val sealed = cipher.iv + cipher.doFinal(key)
            invoke.resolve(JSObject().apply { put("value", Base64.encodeToString(sealed, Base64.NO_WRAP)) })
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot seal the key")
        }
    }

    @Command
    fun openKey(invoke: Invoke) {
        try {
            val sealed = Base64.decode(invoke.parseArgs(KeyArgs::class.java).value, Base64.NO_WRAP)
            val (iv, ciphertext) = splitSealed(sealed) ?: throw IllegalArgumentException("not a sealed key")
            val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply {
                init(Cipher.DECRYPT_MODE, keystoreKey(), GCMParameterSpec(128, iv))
            }
            invoke.resolve(JSObject().apply { put("value", Base64.encodeToString(cipher.doFinal(ciphertext), Base64.NO_WRAP)) })
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot open the key")
        }
    }

    /** This device's FCM token, for the router to wake it (M4). */
    @Command
    fun pushToken(invoke: Invoke) {
        try {
            FirebaseMessaging.getInstance().token.addOnCompleteListener { task ->
                if (task.isSuccessful && task.result != null) {
                    invoke.resolve(JSObject().apply { put("token", task.result) })
                } else {
                    invoke.reject(task.exception?.message ?: "no push token")
                }
            }
        } catch (error: Exception) {
            // No Firebase configuration in this build.
            invoke.reject(error.message ?: "push is not configured")
        }
    }

    /** Android 13 and up ask before showing notifications. */
    @Command
    fun requestNotifications(invoke: Invoke) {
        val needed = Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(activity, "android.permission.POST_NOTIFICATIONS") != PackageManager.PERMISSION_GRANTED
        if (needed) activity.requestPermissions(arrayOf("android.permission.POST_NOTIFICATIONS"), 4242)
        invoke.resolve()
    }

    /** Starts the app again from its launcher activity, in a fresh process (§60). */
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
    fun stopRinging(invoke: Invoke) {
        activity.getSystemService(NotificationManager::class.java)?.cancel(CALL_NOTIFICATION)
        silence()
        invoke.resolve()
    }

    private fun silence() {
        ringtone?.stop()
        ringtone = null
        vibrator?.cancel()
        vibrator = null
    }

    private fun phoneVibrator(): Vibrator? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            activity.getSystemService(VibratorManager::class.java)?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            activity.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        }

    /** The system share sheet (WhatsApp, Signal, mail…) with a text, such as the card link (§32). */
    @Command
    fun shareText(invoke: Invoke) {
        try {
            val text = shareableText(invoke.parseArgs(ShareTextArgs::class.java).text)
            if (text == null) {
                invoke.reject("nothing to share")
                return
            }
            val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
            activity.startActivity(Intent.createChooser(send, null))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot share")
        }
    }

    /**
     * Picks files with the system picker and copies them into the app's folder. The WebView's own
     * file input opens a screen the user cannot come back from without picking something; this one
     * returns to the app either way.
     */
    @Command
    fun pickFiles(invoke: Invoke) {
        val accept = try {
            invoke.parseArgs(PickArgs::class.java).accept
        } catch (_: Exception) {
            ""
        }
        val intent = if (usesPhotoPicker(accept, Build.VERSION.SDK_INT)) {
            // A sheet over the app: the user closes it and is still here, with nothing picked.
            Intent(MediaStore.ACTION_PICK_IMAGES).apply {
                type = accept
                putExtra(MediaStore.EXTRA_PICK_IMAGES_MAX, PICK_LIMIT)
            }
        } else {
            Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = documentType(accept)
                putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
            }
        }
        startActivityForResult(invoke, intent, "picked")
    }

    @ActivityCallback
    fun picked(invoke: Invoke, result: ActivityResult) {
        val picked = JSArray()
        try {
            val data = result.data
            val uris = mutableListOf<android.net.Uri>()
            data?.clipData?.let { clip -> for (index in 0 until clip.itemCount) uris.add(clip.getItemAt(index).uri) }
            data?.data?.let(uris::add)
            for (uri in uris) {
                copyIn(uri)?.let(picked::put)
            }
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot read what was picked")
            return
        }
        invoke.resolve(JSObject().apply { put("files", picked) })
    }

    /** Copies what was picked into the app's folder, and says what it is. */
    private fun copyIn(uri: android.net.Uri): JSObject? {
        val resolver = activity.contentResolver
        var name = "file"
        var size = 0L
        resolver.query(uri, null, null, null, null)?.use { cursor ->
            val nameColumn = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
            val sizeColumn = cursor.getColumnIndex(android.provider.OpenableColumns.SIZE)
            if (cursor.moveToFirst()) {
                if (nameColumn >= 0) name = pickedName(cursor.getString(nameColumn))
                if (sizeColumn >= 0 && !cursor.isNull(sizeColumn)) size = cursor.getLong(sizeColumn)
            }
        }
        val folder = File(activity.filesDir, PICKED_FOLDER).apply { mkdirs() }
        val target = File(folder, "${System.currentTimeMillis()}-${name.replace('/', '_')}")
        resolver.openInputStream(uri)?.use { input ->
            target.outputStream().use { output -> input.copyTo(output) }
        } ?: return null
        return JSObject().apply {
            put("path", target.absolutePath)
            put("name", name)
            put("mime", pickedMime(resolver.getType(uri)))
            put("size", if (size > 0) size else target.length())
        }
    }

    @Command
    fun openFile(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(OpenFileArgs::class.java)
            val uri = FileProvider.getUriForFile(activity, fileProviderAuthority(activity.packageName), File(args.path))
            val view = Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, args.mime)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            activity.startActivity(Intent.createChooser(view, null))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot open the file")
        }
    }

    /** Hands a file of the app to another app through the share sheet; the lending is read-only. */
    @Command
    fun shareFile(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SaveFileArgs::class.java)
            val uri = FileProvider.getUriForFile(activity, fileProviderAuthority(activity.packageName), File(args.path))
            val send = Intent(Intent.ACTION_SEND)
                .setType(args.mime)
                .putExtra(Intent.EXTRA_STREAM, uri)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            activity.startActivity(Intent.createChooser(send, null))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot share the file")
        }
    }

    /**
     * The yearly subscription (§45, §47). Google Play Billing needs a product set up in the Play
     * Console, which only exists once the app is published: until then this says so instead of
     * pretending. Nothing about the payment ever reaches FlickerTalk.
     */
    @Command
    fun subscribe(invoke: Invoke) {
        invoke.reject("paying is not available in this version yet")
    }

    /** What the Store already knows: nothing, while there is no product to know about. */
    @Command
    fun subscription(invoke: Invoke) {
        invoke.resolve(JSObject().apply { put("until", 0) })
    }

    @Command
    fun saveToDownloads(invoke: Invoke) {
        if (!canSaveToDownloads(Build.VERSION.SDK_INT)) {
            invoke.reject("saving to Downloads needs Android 10")
            return
        }
        try {
            val args = invoke.parseArgs(SaveFileArgs::class.java)
            val resolver = activity.contentResolver
            val details = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, args.name)
                put(MediaStore.Downloads.MIME_TYPE, args.mime)
                put(MediaStore.Downloads.IS_PENDING, 1)
            }
            val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, details)
                ?: throw IllegalStateException("Downloads is not available")
            resolver.openOutputStream(uri).use { output ->
                File(args.path).inputStream().use { input -> input.copyTo(output!!) }
            }
            resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot save the file")
        }
    }

    /// Hands a file to the phone's print service. The user picks the printer; nothing is printed
    /// on its own, and the file stays in the app's folder until the service has read it (§53).
    @Command
    fun printFile(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SaveFileArgs::class.java)
            val file = File(args.path)
            if (!file.exists()) throw IllegalStateException("there is nothing to print")
            activity.runOnUiThread {
                if (args.mime.startsWith("image/")) {
                    PrintHelper(activity).apply { scaleMode = PrintHelper.SCALE_MODE_FIT }
                        .printBitmap(args.name, BitmapFactory.decodeFile(file.path))
                } else {
                    val manager = activity.getSystemService(Context.PRINT_SERVICE) as PrintManager
                    manager.print(args.name, FileToPrint(args.name, file), null)
                }
            }
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "cannot print the file")
        }
    }
}

/// A file of the app handed to the print service as it is (a PDF, as the plugins make them).
private class FileToPrint(private val name: String, private val file: File) : PrintDocumentAdapter() {
    override fun onLayout(
        old: PrintAttributes?,
        new: PrintAttributes?,
        signal: CancellationSignal?,
        callback: LayoutResultCallback,
        extras: Bundle?,
    ) {
        if (signal?.isCanceled == true) {
            callback.onLayoutCancelled()
            return
        }
        val info = PrintDocumentInfo.Builder(name)
            .setContentType(PrintDocumentInfo.CONTENT_TYPE_DOCUMENT)
            .setPageCount(PrintDocumentInfo.PAGE_COUNT_UNKNOWN)
            .build()
        callback.onLayoutFinished(info, true)
    }

    override fun onWrite(
        pages: Array<out PageRange>?,
        destination: ParcelFileDescriptor,
        signal: CancellationSignal?,
        callback: WriteResultCallback,
    ) {
        try {
            file.inputStream().use { input ->
                java.io.FileOutputStream(destination.fileDescriptor).use { output -> input.copyTo(output) }
            }
            callback.onWriteFinished(arrayOf(PageRange.ALL_PAGES))
        } catch (error: Exception) {
            callback.onWriteFailed(error.message)
        }
    }
}
