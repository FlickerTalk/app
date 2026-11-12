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

// The WebView applies the range start again to the stream it gets back, so serve from byte 0 to the
// range end and let it skip. Tauri's own ranged answers came out shifted.

private const val ASSET_HOST = "asset.localhost"

data class AssetRange(val file: File, val bytes: LongRange, val length: Long) {
    // no Content-Length, the WebView adds its own from the range
    fun headers(): Map<String, String> = mapOf(
        "Content-Range" to "bytes ${bytes.first}-${bytes.last}/$length",
        "Accept-Ranges" to "bytes",
        "Cache-Control" to "no-store",
    )

    fun stream(): InputStream = FileUpTo(file, bytes.last)
}

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

fun assetRange(url: String, method: String, headers: Map<String, String>, filesDir: File): AssetRange? {
    if (method != "GET") return null
    val asked = headers.entries.firstOrNull { it.key.equals("Range", ignoreCase = true) }?.value ?: return null
    val file = assetFile(url, filesDir) ?: return null
    val length = file.length()
    val bytes = byteRange(asked, length) ?: return null
    return AssetRange(file, bytes, length)
}

// resolve links before checking, so one can't point outside files/
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

// webView.webViewClient only exists from Android 8
fun serveAssetRanges(webView: WebView, filesDir: File) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
    val current = webView.webViewClient
    if (current is AssetRangeClient) return
    webView.webViewClient = AssetRangeClient(current, filesDir)
}
