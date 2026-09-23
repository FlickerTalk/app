package com.flickertalk.platform

import android.app.ActivityManager
import android.media.AudioManager
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

class PlatformPluginTest {
    // Must match `android:authorities` in the plugin's AndroidManifest.xml.
    @Test
    fun theFileProviderAuthorityBelongsToTheApp() {
        assertEquals("com.flickertalk.app.ft.files", fileProviderAuthority("com.flickertalk.app"))
    }

    // Android 10 and up write to Downloads without any storage permission.
    @Test
    fun downloadsNeedAndroid10() {
        assertEquals(false, canSaveToDownloads(28))
        assertEquals(true, canSaveToDownloads(29))
    }

    // Asking for pictures opens the photo picker, which is a sheet over the app and closes with
    // a swipe: the user is never taken out of FlickerTalk (Ioan, 2026-09-23). Anything else is a
    // document, and that is the system's own picker.
    @Test
    fun picturesUseThePhotoPickerWhereThereIsOne() {
        assertEquals(true, usesPhotoPicker("image/*", 33))
        assertEquals(true, usesPhotoPicker("image/jpeg", 36))
        assertEquals(false, usesPhotoPicker("image/*", 32))
        assertEquals(false, usesPhotoPicker("", 36))
        assertEquals(false, usesPhotoPicker("application/pdf", 36))
    }

    @Test
    fun theDocumentPickerOnlyShowsWhatWasAskedFor() {
        assertEquals("*/*", documentType(""))
        assertEquals("application/pdf", documentType("application/pdf"))
        assertEquals("image/*", documentType("image/*"))
        assertEquals("*/*", documentType("nonsense"))
    }

    // An incoming call rings as the user set the phone: sound and vibration, vibration only, or
    // nothing at all.
    @Test
    fun ringingFollowsTheRingerMode() {
        assertEquals(Ringing(sound = true, vibrate = true), ringingFor(AudioManager.RINGER_MODE_NORMAL))
        assertEquals(Ringing(sound = false, vibrate = true), ringingFor(AudioManager.RINGER_MODE_VIBRATE))
        assertEquals(Ringing(sound = false, vibrate = false), ringingFor(AudioManager.RINGER_MODE_SILENT))
    }

    // M4: the router's push only says "wake"; anything else is ignored.
    @Test
    fun onlyWakeUpsCount() {
        assertEquals(true, isWake(mapOf("t" to "wake")))
        assertEquals(false, isWake(mapOf("t" to "other")))
        assertEquals(false, isWake(emptyMap()))
    }

    // An open app is already connected: it gets everything without a notification.
    @Test
    fun notifiesOnlyWhenTheAppIsNotOnScreen() {
        assertEquals(false, shouldNotify(ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND))
        assertEquals(true, shouldNotify(ActivityManager.RunningAppProcessInfo.IMPORTANCE_CACHED))
        assertEquals(true, shouldNotify(ActivityManager.RunningAppProcessInfo.IMPORTANCE_SERVICE))
    }

    // The sealed storage key: the 12-byte GCM nonce, then the ciphertext with its tag.
    @Test
    fun aSealedKeySplitsIntoNonceAndCiphertext() {
        val sealed = ByteArray(12) { 1 } + ByteArray(48) { 2 }
        val (iv, ciphertext) = splitSealed(sealed)!!
        assertEquals(12, iv.size)
        assertEquals(48, ciphertext.size)
        assertEquals(null, splitSealed(ByteArray(12)))
    }

    @Test
    fun onlyRealTextIsShared() {
        assertEquals("Add me: https://flickertalk.com/add#card", shareableText("  Add me: https://flickertalk.com/add#card\n"))
        assertNull(shareableText("   "))
    }

    // §66: a call arriving with the app in the background has to show on the screen, not just
    // ring. Android does it with a call notification and a full-screen intent, on its own channel
    // so the user can keep calls loud and messages quiet.
    @Test
    fun callsHaveTheirOwnNotificationChannel() {
        assertEquals("ft.call", CALL_CHANNEL)
    }

    @Test
    fun theCallNotificationSaysWhoIsCallingAndWhatKind() {
        assertEquals("Ioan", callTitle("Ioan"))
        // A contact with no name is still a caller: the notification says "Someone" in the phone's language.
        assertNull(callTitle("  "))
        assertEquals(R.string.ft_incoming_video_call, callText(true))
        assertEquals(R.string.ft_incoming_call, callText(false))
    }

    // Notifications speak the phone's language, like the app (the same languages as
    // src/i18n/*.json). Every translation has every text of the English one, and none is empty.
    @Test
    fun notificationTextsExistInEveryLanguage() {
        val res = File("src/main/res")
        fun texts(dir: String): Map<String, String> {
            val file = File(res, "$dir/ft_strings.xml")
            assertTrue("missing $file", file.isFile)
            val nodes = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(file)
                .getElementsByTagName("string")
            return (0 until nodes.length).map { nodes.item(it) as Element }
                .associate { it.getAttribute("name") to it.textContent.trim() }
        }
        val english = texts("values")
        assertTrue(english.isNotEmpty())
        val languages = listOf(
            "es", "pt", "fr", "de", "it", "ro", "ru", "uk", "pl", "tr", "ar",
            "hi", "bn", "in", "vi", "th", "ja", "ko", "zh-rCN", "zh-rTW",
        )
        for (language in languages) {
            val translated = texts("values-$language")
            assertEquals(language, english.keys, translated.keys)
            translated.forEach { (name, text) -> assertTrue("$language/$name", text.isNotEmpty()) }
        }
    }

    // The notification's buttons come back as an extra on the intent that opens the app; anything
    // else is nothing at all.
    @Test
    fun onlyAnswerAndDeclineComeFromTheNotification() {
        assertEquals("answer", callAction("answer"))
        assertEquals("decline", callAction("decline"))
        assertEquals("", callAction(null))
        assertEquals("", callAction("do-something-else"))
    }

    // The file picker of the WebView takes the user out of the app; ours copies what was picked
    // into the app's own folder and gives it a name we can show.
    @Test
    fun aPickedFileKeepsItsNameAndGetsAKindWeUnderstand() {
        assertEquals("photo.jpg", pickedName("photo.jpg"))
        assertEquals("something with no name is still a file", "file", pickedName(""))
        assertEquals("file", pickedName(null))
        assertEquals("image/jpeg", pickedMime("image/jpeg"))
        assertEquals("application/octet-stream", pickedMime(null))
    }

    @Test
    fun aPickedFileIsWrittenWhereOnlyTheAppCanRead() {
        assertEquals("uploads", PICKED_FOLDER)
    }

    // Issue app#7: the weekly hours, Monday first, as the core hands them over.
    private val week = "1080-1320;1080-1320;1080-1320;1080-1320;900-1320;all;all"

    @Test
    fun noHoursMeansTheOldBehaviour() {
        assertEquals(true, mayDisturb("", 0, 3 * 60))
    }

    @Test
    fun insideItsStretchADayMayMakeNoise() {
        assertEquals(true, mayDisturb(week, 0, 19 * 60))
        assertEquals(true, mayDisturb(week, 4, 15 * 60))
        assertEquals(true, mayDisturb(week, 6, 3 * 60))
    }

    @Test
    fun outsideItsStretchADayIsQuiet() {
        assertEquals(false, mayDisturb(week, 0, 9 * 60))
        assertEquals(false, mayDisturb(week, 0, 22 * 60))
        assertEquals(false, mayDisturb(week, 3, 14 * 60))
        assertEquals(false, mayDisturb("none;none;none;none;none;none;none", 2, 12 * 60))
    }

    @Test
    fun aStretchCanGoPastMidnight() {
        val nights = "1320-120;all;all;all;all;all;all"
        assertEquals(true, mayDisturb(nights, 0, 23 * 60))
        assertEquals(true, mayDisturb(nights, 0, 60))
        assertEquals(false, mayDisturb(nights, 0, 12 * 60))
    }

    // Issue app#4 and app#7: a muted contact, or quiet hours, show the call but make no noise.
    @Test
    fun aQuietCallNeitherRingsNorVibrates() {
        assertEquals(Ringing(sound = false, vibrate = false), ringingFor(AudioManager.RINGER_MODE_NORMAL, quiet = true))
        assertEquals(Ringing(sound = true, vibrate = true), ringingFor(AudioManager.RINGER_MODE_NORMAL, quiet = false))
    }

    // Issue app#9: a wake says which of the eight capabilities was used. The device's own (0, or
    // none from an older router) always tells; a hidden session's only while it is open.
    @Test
    fun aWakeForTheMainListIsAlwaysHeard() {
        assertEquals(true, wakeIsHeard(null, emptySet()))
        assertEquals(true, wakeIsHeard("0", emptySet()))
    }

    @Test
    fun aWakeForAHiddenSessionIsHeardOnlyWhileItIsOpen() {
        assertEquals(false, wakeIsHeard("3", emptySet()))
        assertEquals(false, wakeIsHeard("3", setOf(1, 2)))
        assertEquals(true, wakeIsHeard("3", setOf(3)))
        assertEquals(false, wakeIsHeard("nonsense", setOf(3)))
    }
}
