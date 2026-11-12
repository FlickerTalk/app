package com.flickertalk.platform

import android.webkit.WebViewClient
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.lang.reflect.Modifier
import java.net.URLEncoder
import java.nio.file.Files

class AssetRangesTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private val content = ByteArray(5000) { (it % 251).toByte() }

    private fun filesDir(): File = tmp.newFolder("files")

    private fun write(dir: File, name: String, bytes: ByteArray = content): File =
        File(dir, name).apply {
            parentFile?.mkdirs()
            writeBytes(bytes)
        }

    private fun urlOf(file: File): String =
        "http://asset.localhost/" + URLEncoder.encode(file.path, "UTF-8").replace("+", "%20")

    private fun readLikeTheWebView(range: AssetRange): ByteArray {
        val stream = range.stream()
        assertTrue(stream.available().toLong() >= range.bytes.last + 1)
        var toSkip = range.bytes.first
        while (toSkip > 0) {
            val skipped = stream.skip(toSkip)
            assertTrue("the stream must skip forward", skipped > 0)
            toSkip -= skipped
        }
        return stream.use { it.readBytes() }
    }

    @Test
    fun aRangeIsOneSpanOfTheFile() {
        assertEquals(0L..999L, byteRange("bytes=0-999", 5000))
        assertEquals(100L..4999L, byteRange("bytes=100-", 5000))
        assertEquals(4500L..4999L, byteRange("bytes=-500", 5000))
        assertEquals(4000L..4999L, byteRange("bytes=4000-9000", 5000))
        assertEquals(0L..4999L, byteRange("bytes=-9000", 5000))
        assertEquals(10L..20L, byteRange(" bytes = 10 - 20 ", 5000))
    }

    @Test
    fun anythingElseIsLeftToTauri() {
        assertNull(byteRange("bytes=5000-", 5000))
        assertNull(byteRange("bytes=10-5", 5000))
        assertNull(byteRange("bytes=0-1,5-6", 5000))
        assertNull(byteRange("items=0-1", 5000))
        assertNull(byteRange("bytes=-0", 5000))
        assertNull(byteRange("bytes=-", 5000))
        assertNull(byteRange("bytes=abc-", 5000))
        assertNull(byteRange("bytes=0-", 0))
    }

    @Test
    fun theWebViewGetsExactlyTheBytesAskedFor() {
        val dir = filesDir()
        val file = write(dir, "01a134de/cascada.mp4")
        for (asked in listOf("bytes=0-", "bytes=0-999", "bytes=10-1000", "bytes=4096-", "bytes=-300")) {
            val range = assetRange(urlOf(file), "GET", mapOf("Range" to asked), dir)
            assertNotNull(asked, range)
            val bytes = range!!.bytes
            assertArrayEquals(asked, content.copyOfRange(bytes.first.toInt(), bytes.last.toInt() + 1), readLikeTheWebView(range))
        }
    }

    @Test
    fun theHeadersSayWhichBytesTheyAre() {
        val dir = filesDir()
        val file = write(dir, "clip.webm")
        val range = assetRange(urlOf(file), "GET", mapOf("range" to "bytes=10-1000"), dir)!!
        assertEquals(file.canonicalFile, range.file)
        assertEquals(5000L, range.length)
        assertEquals(
            mapOf(
                "Content-Range" to "bytes 10-1000/5000",
                "Accept-Ranges" to "bytes",
                "Cache-Control" to "no-store",
            ),
            range.headers(),
        )
    }

    @Test
    fun anyFileOfTheAppIsServed() {
        val dir = filesDir()
        val voice = write(dir, "outgoing/114a2ab8e0006949e0de94ca5db6cead")
        assertEquals(voice.canonicalFile, assetRange(urlOf(voice), "GET", mapOf("Range" to "bytes=0-"), dir)?.file)
        val spaced = write(dir, "01a1/my clip (1).mov")
        assertEquals(spaced.canonicalFile, assetRange(urlOf(spaced), "GET", mapOf("Range" to "bytes=0-"), dir)?.file)
    }

    @Test
    fun nothingOutsideTheFilesFolderIsServed() {
        val dir = filesDir()
        val range = mapOf("Range" to "bytes=0-")
        val outside = write(tmp.root, "flickertalk.db")
        assertNull(assetRange(urlOf(outside), "GET", range, dir))
        assertNull(assetRange(urlOf(File(dir, "../flickertalk.db")), "GET", range, dir))
        assertNull(assetRange(urlOf(write(dir, ".hidden")), "GET", range, dir))
        assertNull(assetRange(urlOf(write(dir, ".cache/clip.mp4")), "GET", range, dir))
        assertNull(assetRange(urlOf(File(dir, "missing.mp4")), "GET", range, dir))
        assertNull(assetRange(urlOf(File(dir, "outgoing")), "GET", range, dir))
        assertNull(assetRange(urlOf(dir), "GET", range, dir))
        val link = File(dir, "link.mp4")
        Files.createSymbolicLink(link.toPath(), outside.toPath())
        assertNull(assetRange(urlOf(link), "GET", range, dir))
    }

    @Test
    fun otherRequestsAreLeftToTauri() {
        val dir = filesDir()
        val file = write(dir, "clip.mp4")
        val url = urlOf(file)
        val range = mapOf("Range" to "bytes=0-")
        assertNull(assetRange(url, "GET", emptyMap(), dir))
        assertNull(assetRange(url, "HEAD", range, dir))
        assertNull(assetRange(url.replace("asset.localhost", "tauri.localhost"), "GET", range, dir))
        assertNull(assetRange("ftplugin://localhost/index.html", "GET", range, dir))
        assertNull(assetRange("http://asset.localhost/%zz", "GET", range, dir))
        assertNull(assetRange("http://asset.localhost/not absolute", "GET", range, dir))
        assertNull(assetRange(url, "GET", mapOf("Range" to "bytes=0-1,5-6"), dir))
    }

    @Test
    fun everyCallReachesTheClientItWraps() {
        val wrapped = AssetRangeClient::class.java
        val missing = WebViewClient::class.java.declaredMethods
            .filter { Modifier.isPublic(it.modifiers) && !Modifier.isStatic(it.modifiers) && !Modifier.isFinal(it.modifiers) }
            .filter { method ->
                runCatching { wrapped.getDeclaredMethod(method.name, *method.parameterTypes) }.isFailure
            }
            .map { it.name + it.parameterTypes.joinToString(prefix = "(", postfix = ")") { type -> type.simpleName } }
        assertEquals(emptyList<String>(), missing)
    }
}
