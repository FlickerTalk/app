package com.flickertalk.platform

import android.annotation.TargetApi
import android.graphics.Bitmap
import android.net.http.SslError
import android.os.Build
import android.os.Message
import android.view.KeyEvent
import android.webkit.ClientCertRequest
import android.webkit.HttpAuthHandler
import android.webkit.MimeTypeMap
import android.webkit.RenderProcessGoneDetail
import android.webkit.SafeBrowsingResponse
import android.webkit.SslErrorHandler
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.RandomAccessFile
import java.net.URI
import java.net.URISyntaxException

// ---- Byte ranges of the app's own files (2026-10-13, videos in the chat) ----
// A video or a voice message plays from Tauri's asset protocol (`http://asset.localhost/<path>`),
// which answers a range request with just the bytes asked for. But the Android WebView applies the
// request's range again to the stream it is handed: it skips the range's start in it. The first
// piece (from byte 0) arrived, and every later piece came shifted or failed, so a video bigger
// than its first piece never played (seen on a Samsung with Android 13). Here a range request for
// a file of the app is answered with the file itself, from its first byte and cut at the range's
// end, and the WebView skips to the range's start: read from the disk as it plays, never in memory.

private const val ASSET_HOST = "asset.localhost"

/** The [bytes] (first and last included) of a [file] of [length] bytes that a request asked for. */
data class AssetRange(val file: File, val bytes: LongRange, val length: Long) {
    /**
     * No `Content-Length`: the WebView adds it from the range (ours came out twice). No CORS
     * header either: ranges are asked for by `<video>` and `<audio>`, which load without CORS, and
     * nothing in the app reads these files with `fetch` (the CSP's `connect-src` does not allow it).
     */
    fun headers(): Map<String, String> = mapOf(
        "Content-Range" to "bytes ${bytes.first}-${bytes.last}/$length",
        "Accept-Ranges" to "bytes",
        // As wry does for everything it answers.
        "Cache-Control" to "no-store",
    )

    /** The file from its first byte up to the range's last one: the WebView skips to the start. */
    fun stream(): InputStream = FileUpTo(file, bytes.last)
}

/**
 * The bytes a `Range` header asks for in a file of [length] bytes, first and last included: one
 * range, `bytes=a-b`, `bytes=a-` or `bytes=-n`. Anything else (several ranges, a start past the
 * end, another unit) is null and left to Tauri.
 */
fun byteRange(header: String, length: Long): LongRange? {
    if (length <= 0) return null
    val spec = header.trim()
    val equals = spec.indexOf('=')
    if (equals < 0 || spec.substring(0, equals).trim() != "bytes") return null
    val value = spec.substring(equals + 1).trim()
    val dash = value.indexOf('-')
    if (dash < 0 || value.contains(',')) return null
    val first = value.substring(0, dash).trim()
    val last = value.substring(dash + 1).trim()
    if (first.isEmpty()) {
        val suffix = last.toLongOrNull()?.takeIf { it > 0 } ?: return null
        return maxOf(0L, length - suffix)..(length - 1)
    }
    val start = first.toLongOrNull()?.takeIf { it in 0 until length } ?: return null
    val end = if (last.isEmpty()) length - 1 else (last.toLongOrNull() ?: return null).coerceAtMost(length - 1)
    return if (end < start) null else start..end
}

/**
 * What a request to the asset protocol asks for, when it is a `GET` with one byte range of a file
 * of the app. Null leaves the request to Tauri as before (pictures, which ask for the whole file).
 */
fun assetRange(url: String, method: String, headers: Map<String, String>, filesDir: File): AssetRange? {
    if (method != "GET") return null
    val asked = headers.entries.firstOrNull { it.key.equals("Range", ignoreCase = true) }?.value ?: return null
    val file = assetFile(url, filesDir) ?: return null
    val length = file.length()
    val bytes = byteRange(asked, length) ?: return null
    return AssetRange(file, bytes, length)
}

/**
 * The file an asset URL names (Tauri: the absolute path, percent-encoded, after the host), only if
 * Tauri's asset scope already allows it: a file anywhere under `files/` (the scope in
 * `tauri.conf.json`), none of its folders hidden. Links are followed before deciding, so one cannot point out of the folder.
 */
private fun assetFile(url: String, filesDir: File): File? {
    val uri = try {
        URI(url)
    } catch (e: URISyntaxException) {
        return null
    }
    if (uri.scheme != "http" || uri.host != ASSET_HOST) return null
    val file = File(uri.path?.removePrefix("/") ?: return null)
    if (!file.isAbsolute) return null
    return try {
        val root = filesDir.canonicalFile
        val canonical = file.canonicalFile
        if (!canonical.path.startsWith(root.path + File.separator)) return null
        val inside = canonical.path.substring(root.path.length + 1)
        if (inside.split(File.separatorChar).any { it.startsWith(".") }) return null
        canonical.takeIf { it.isFile }
    } catch (e: IOException) {
        null
    }
}

/** A file from its first byte up to [last], included, that can skip forward without reading. */
private class FileUpTo(file: File, last: Long) : InputStream() {
    private val input = RandomAccessFile(file, "r")
    private val end = last + 1
    private var position = 0L

    override fun read(): Int {
        if (position >= end) return -1
        val byte = input.read()
        if (byte >= 0) position += 1
        return byte
    }

    override fun read(buffer: ByteArray, offset: Int, count: Int): Int {
        if (count == 0) return 0
        val left = end - position
        if (left <= 0) return -1
        val read = input.read(buffer, offset, minOf(count.toLong(), left).toInt())
        if (read > 0) position += read
        return read
    }

    override fun skip(count: Long): Long {
        val skipped = count.coerceIn(0L, maxOf(0L, end - position))
        position += skipped
        input.seek(position)
        return skipped
    }

    override fun available(): Int = (end - position).coerceIn(0L, Int.MAX_VALUE.toLong()).toInt()

    override fun close() = input.close()
}

/**
 * wry's WebViewClient with the range requests above answered first. Every other call the WebView
 * makes goes to wry's client unchanged (a test checks that none is left out).
 */
class AssetRangeClient(private val wry: WebViewClient, private val filesDir: File) : WebViewClient() {
    override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
        ranged(request) ?: wry.shouldInterceptRequest(view, request)

    private fun ranged(request: WebResourceRequest): WebResourceResponse? {
        val range = assetRange(request.url.toString(), request.method, request.requestHeaders ?: emptyMap(), filesDir) ?: return null
        val stream = try {
            range.stream()
        } catch (e: IOException) {
            return null
        }
        val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(range.file.extension.lowercase()) ?: "application/octet-stream"
        return WebResourceResponse(mime, null, 206, "Partial Content", range.headers(), stream)
    }

    @Suppress("OverridingDeprecatedMember", "DEPRECATION")
    override fun shouldInterceptRequest(view: WebView, url: String): WebResourceResponse? = wry.shouldInterceptRequest(view, url)

    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean = wry.shouldOverrideUrlLoading(view, request)

    @Suppress("OverridingDeprecatedMember", "DEPRECATION")
    override fun shouldOverrideUrlLoading(view: WebView, url: String): Boolean = wry.shouldOverrideUrlLoading(view, url)

    override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) = wry.onPageStarted(view, url, favicon)

    override fun onPageFinished(view: WebView, url: String) = wry.onPageFinished(view, url)

    override fun onLoadResource(view: WebView, url: String) = wry.onLoadResource(view, url)

    override fun onPageCommitVisible(view: WebView, url: String) = wry.onPageCommitVisible(view, url)

    @Suppress("OverridingDeprecatedMember", "DEPRECATION")
    override fun onTooManyRedirects(view: WebView, cancelMsg: Message, continueMsg: Message) = wry.onTooManyRedirects(view, cancelMsg, continueMsg)

    @Suppress("OverridingDeprecatedMember", "DEPRECATION")
    override fun onReceivedError(view: WebView, errorCode: Int, description: String, failingUrl: String) =
        wry.onReceivedError(view, errorCode, description, failingUrl)

    override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) = wry.onReceivedError(view, request, error)

    override fun onReceivedHttpError(view: WebView, request: WebResourceRequest, errorResponse: WebResourceResponse) =
        wry.onReceivedHttpError(view, request, errorResponse)

    override fun onFormResubmission(view: WebView, dontResend: Message, resend: Message) = wry.onFormResubmission(view, dontResend, resend)

    override fun doUpdateVisitedHistory(view: WebView, url: String, isReload: Boolean) = wry.doUpdateVisitedHistory(view, url, isReload)

    override fun onReceivedSslError(view: WebView, handler: SslErrorHandler, error: SslError) = wry.onReceivedSslError(view, handler, error)

    override fun onReceivedClientCertRequest(view: WebView, request: ClientCertRequest) = wry.onReceivedClientCertRequest(view, request)

    override fun onReceivedHttpAuthRequest(view: WebView, handler: HttpAuthHandler, host: String, realm: String) =
        wry.onReceivedHttpAuthRequest(view, handler, host, realm)

    override fun shouldOverrideKeyEvent(view: WebView, event: KeyEvent): Boolean = wry.shouldOverrideKeyEvent(view, event)

    override fun onUnhandledKeyEvent(view: WebView, event: KeyEvent) = wry.onUnhandledKeyEvent(view, event)

    override fun onScaleChanged(view: WebView, oldScale: Float, newScale: Float) = wry.onScaleChanged(view, oldScale, newScale)

    override fun onReceivedLoginRequest(view: WebView, realm: String, account: String?, args: String) =
        wry.onReceivedLoginRequest(view, realm, account, args)

    override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean = wry.onRenderProcessGone(view, detail)

    @TargetApi(Build.VERSION_CODES.O_MR1)
    override fun onSafeBrowsingHit(view: WebView, request: WebResourceRequest, threatType: Int, callback: SafeBrowsingResponse) =
        wry.onSafeBrowsingHit(view, request, threatType, callback)
}

/**
 * Puts [AssetRangeClient] in front of the client wry gave [webView], once. Android 8 and later:
 * before it a WebView cannot hand its client back, and videos stay as they were.
 */
fun serveAssetRanges(webView: WebView, filesDir: File) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
    val current = webView.webViewClient
    if (current is AssetRangeClient) return
    webView.webViewClient = AssetRangeClient(current, filesDir)
}
