package com.flickertalk.platform

import android.app.Activity
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.BitmapFactory
import android.os.Build
import android.os.Bundle
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintDocumentInfo
import android.print.PrintManager
import android.provider.MediaStore
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import androidx.print.PrintHelper
import app.tauri.annotation.InvokeArg
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import java.io.File

// own subclass, wry already declares androidx's FileProvider and the manifest can't merge two of one class
class FtFileProvider : FileProvider()

const val PICK_LIMIT = 20

fun fileProviderAuthority(packageName: String): String = "$packageName.ft.files"

fun canSaveToDownloads(sdk: Int): Boolean = sdk >= Build.VERSION_CODES.Q

fun usesPhotoPicker(accept: String, sdk: Int): Boolean =
    (accept.startsWith("image/") || accept.startsWith("video/")) && sdk >= Build.VERSION_CODES.TIRAMISU

// null = both
fun photoPickerType(accept: String): String? =
    accept.takeIf { it == "image/*" || it == "video/*" || Regex("^(image|video)/[a-z0-9.+-]+$").matches(it) }

fun documentType(accept: String): String =
    if (Regex("^[a-z]+/([a-z0-9.+-]+|\\*)$").matches(accept)) accept else "*/*"

const val PICKED_FOLDER = "uploads"

fun photoName(now: Long, zone: java.util.TimeZone = java.util.TimeZone.getDefault()): String {
    val format = java.text.SimpleDateFormat("'photo-'yyyyMMdd-HHmmss'.jpg'", java.util.Locale.ROOT)
    format.timeZone = zone
    return format.format(java.util.Date(now))
}

// same naming as iOS. A name must never become a path.
fun pickedBase(now: Long, index: Int, name: String): String = "$now-$index-${name.replace('/', '_')}"

// never reuse an existing file, a message may point to it
fun newPickedFile(folder: File, base: String): File {
    val dot = base.lastIndexOf('.').takeIf { it > 0 }
    val (stem, extension) = if (dot == null) base to "" else base.substring(0, dot) to base.substring(dot)
    var attempt = 1
    while (true) {
        val file = File(folder, if (attempt == 1) base else "$stem-$attempt$extension")
        if (file.createNewFile()) return file
        attempt += 1
    }
}

fun keepsPhoto(resultCode: Int, size: Long): Boolean = resultCode == Activity.RESULT_OK && size > 0

fun pickedName(name: String?): String = name?.trim()?.takeIf { it.isNotEmpty() } ?: "file"

fun pickedMime(mime: String?): String = mime?.trim()?.takeIf { it.isNotEmpty() } ?: "application/octet-stream"

fun shareableText(text: String): String? = text.trim().ifEmpty { null }

fun pickIntent(accept: String): Intent =
    if (usesPhotoPicker(accept, Build.VERSION.SDK_INT)) {
        Intent(MediaStore.ACTION_PICK_IMAGES).apply {
            photoPickerType(accept)?.let { type = it }
            putExtra(MediaStore.EXTRA_PICK_IMAGES_MAX, PICK_LIMIT)
        }
    } else {
        Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = documentType(accept)
            putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
        }
    }

@InvokeArg
class ShareTextArgs {
    lateinit var text: String
}

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

class Files(private val activity: Activity) {
    private var pendingPhoto: File? = null

    fun shareText(text: String) {
        val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
        activity.startActivity(Intent.createChooser(send, null))
    }

    // with CAMERA declared (for calls) the capture intent is refused until it's granted
    fun photoIntent(): Intent? {
        if (ContextCompat.checkSelfPermission(activity, android.Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            activity.requestPermissions(arrayOf(android.Manifest.permission.CAMERA), 4243)
            return null
        }
        val folder = File(activity.filesDir, PICKED_FOLDER).apply { mkdirs() }
        val target = newPickedFile(folder, photoName(System.currentTimeMillis()))
        pendingPhoto = target
        val uri = FileProvider.getUriForFile(activity, fileProviderAuthority(activity.packageName), target)
        return Intent(MediaStore.ACTION_IMAGE_CAPTURE)
            .putExtra(MediaStore.EXTRA_OUTPUT, uri)
            .addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }

    fun photoTaken(resultCode: Int): JSArray {
        val target = pendingPhoto
        pendingPhoto = null
        val files = JSArray()
        if (target != null) {
            if (keepsPhoto(resultCode, target.length())) {
                files.put(JSObject().apply {
                    put("path", target.absolutePath)
                    put("name", target.name)
                    put("mime", "image/jpeg")
                    put("size", target.length())
                })
            } else {
                target.delete()
            }
        }
        return files
    }

    fun copyPicked(data: Intent?): JSArray {
        val picked = JSArray()
        val uris = mutableListOf<android.net.Uri>()
        data?.clipData?.let { clip -> for (index in 0 until clip.itemCount) uris.add(clip.getItemAt(index).uri) }
        data?.data?.let(uris::add)
        val now = System.currentTimeMillis()
        for ((index, uri) in uris.withIndex()) {
            copyIn(uri, now, index)?.let(picked::put)
        }
        return picked
    }

    private fun copyIn(uri: android.net.Uri, now: Long, index: Int): JSObject? {
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
        val input = resolver.openInputStream(uri) ?: return null
        val target = newPickedFile(folder, pickedBase(now, index, name))
        input.use { target.outputStream().use { output -> it.copyTo(output) } }
        return JSObject().apply {
            put("path", target.absolutePath)
            put("name", name)
            put("mime", pickedMime(resolver.getType(uri)))
            put("size", if (size > 0) size else target.length())
        }
    }

    fun openFile(path: String, mime: String) {
        val uri = FileProvider.getUriForFile(activity, fileProviderAuthority(activity.packageName), File(path))
        val view = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, mime)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        activity.startActivity(Intent.createChooser(view, null))
    }

    fun shareFile(path: String, mime: String) {
        val uri = FileProvider.getUriForFile(activity, fileProviderAuthority(activity.packageName), File(path))
        val send = Intent(Intent.ACTION_SEND)
            .setType(mime)
            .putExtra(Intent.EXTRA_STREAM, uri)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        activity.startActivity(Intent.createChooser(send, null))
    }

    fun saveToDownloads(path: String, name: String, mime: String) {
        val resolver = activity.contentResolver
        val details = ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, name)
            put(MediaStore.Downloads.MIME_TYPE, mime)
            put(MediaStore.Downloads.IS_PENDING, 1)
        }
        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, details)
            ?: throw IllegalStateException("Downloads is not available")
        resolver.openOutputStream(uri).use { output ->
            File(path).inputStream().use { input -> input.copyTo(output!!) }
        }
        resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
    }

    fun printFile(path: String, name: String, mime: String) {
        val file = File(path)
        if (!file.exists()) throw IllegalStateException("there is nothing to print")
        activity.runOnUiThread {
            if (mime.startsWith("image/")) {
                PrintHelper(activity).apply { scaleMode = PrintHelper.SCALE_MODE_FIT }
                    .printBitmap(name, BitmapFactory.decodeFile(file.path))
            } else {
                val manager = activity.getSystemService(Context.PRINT_SERVICE) as PrintManager
                manager.print(name, FileToPrint(name, file), null)
            }
        }
    }
}

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
