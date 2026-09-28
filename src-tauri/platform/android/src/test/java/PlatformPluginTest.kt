package com.flickertalk.platform

import android.app.ActivityManager
import android.content.pm.ServiceInfo
import android.media.AudioManager
import com.android.billingclient.api.Purchase
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
        assertEquals(true, usesPhotoPicker("video/*", 33))
        assertEquals(true, usesPhotoPicker("image/*,video/*", 33))
        assertEquals(false, usesPhotoPicker("image/*", 32))
        assertEquals(false, usesPhotoPicker("", 36))
        assertEquals(false, usesPhotoPicker("application/pdf", 36))
    }

    // The photo picker shows pictures, videos or both; a type it does not take means both.
    @Test
    fun thePhotoPickerShowsPicturesOrVideosOrBoth() {
        assertEquals("image/*", photoPickerType("image/*"))
        assertEquals("video/*", photoPickerType("video/*"))
        assertEquals(null, photoPickerType("image/*,video/*"))
        assertEquals(null, photoPickerType(""))
    }

    // A photo taken with the camera is named like a voice note, by when it was taken; one the
    // user backed out of, or that came out empty, is not kept (Ioan, 2026-09-23).
    @Test
    fun aPhotoIsNamedByWhenItWasTakenAndKeptOnlyWhenItCameOut() {
        assertEquals("photo-20260923-190501.jpg", photoName(1790183101670L, java.util.TimeZone.getTimeZone("Europe/Madrid")))
        assertEquals(true, keepsPhoto(android.app.Activity.RESULT_OK, 117_130L))
        assertEquals(false, keepsPhoto(android.app.Activity.RESULT_OK, 0L))
        assertEquals(false, keepsPhoto(android.app.Activity.RESULT_CANCELED, 117_130L))
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

    // What the Store handed back about one purchase, as the plugin reads it.
    private fun purchase(
        product: String = YEARLY,
        state: Int = Purchase.PurchaseState.PURCHASED,
        boughtAt: Long = 1790208000000L,
        acknowledged: Boolean = true,
    ) = StorePurchase(product, state, boughtAt, acknowledged)

    // The Store says when the money was taken, never until when. Asking Play's server for the
    // expiry would mean our backend learning who pays (§45-46), so the phone counts a year from
    // the purchase; every renewal moves that date on.
    @Test
    fun aPurchaseIsGoodForAYear() {
        val bought = 1790208000000L
        assertEquals(bought + 365L * 24 * 60 * 60 * 1000, untilFromPurchase(bought))
    }

    // Play takes payments that land days later (cash, transfer). Pending is not paid for, and the
    // app never pretends it is (§84).
    @Test
    fun aPendingPurchaseBuysNothingYet() {
        assertEquals(0L, activeUntil(listOf(purchase(state = Purchase.PurchaseState.PENDING))))
    }

    @Test
    fun nothingBoughtIsNoSubscription() {
        assertEquals(0L, activeUntil(emptyList()))
    }

    // Whatever else the Play account carries, only our own product pays for FlickerTalk.
    @Test
    fun onlyTheYearlyProductCounts() {
        assertEquals(0L, activeUntil(listOf(purchase(product = "com.someone.else.pro"))))
    }

    // A renewal comes back as a later purchase time: the phone follows the furthest one.
    @Test
    fun theFurthestPurchaseIsTheOneThatCounts() {
        val first = 1790208000000L
        val renewed = first + 365L * 24 * 60 * 60 * 1000
        val until = activeUntil(listOf(purchase(boughtAt = renewed), purchase(boughtAt = first)))
        assertEquals(untilFromPurchase(renewed), until)
    }

    // Google gives the money back if a purchase is not acknowledged within three days, so it is
    // acknowledged once and only once, and never while it is still pending.
    @Test
    fun aPaidPurchaseIsAcknowledgedOnce() {
        assertEquals(true, needsAcknowledgement(purchase(acknowledged = false)))
        assertEquals(false, needsAcknowledgement(purchase(acknowledged = true)))
        assertEquals(
            false,
            needsAcknowledgement(purchase(state = Purchase.PurchaseState.PENDING, acknowledged = false)),
        )
    }

    // Local reminders (2026-09-27): what the core sends is read leniently, each reminder keeps
    // the same alarm slot, and only the ones still ahead are set again after a reboot.
    @Test
    fun remindersAreReadFromTheCoreAndKeepTheirSlot() {
        val entries = parseReminders("""[{"plugin":"com.example.notes","id":"r1","at":5000,"text":"milk"},{"plugin":"","id":"x","at":1},{"id":"no-plugin","at":2},"junk"]""")
        assertEquals(1, entries.size)
        assertEquals(ReminderEntry("com.example.notes", "r1", 5000, "milk"), entries[0])
        assertEquals(0, parseReminders("not json").size)
        assertEquals(reminderRequestCode("com.example.notes", "r1"), reminderRequestCode("com.example.notes", "r1"))
        assertTrue(reminderRequestCode("com.example.notes", "r1") != reminderRequestCode("com.example.notes", "r2"))
        assertTrue(reminderRequestCode("a", "b") >= 1000)
        assertEquals("com.example.notes\nr1", reminderKey("com.example.notes", "r1"))
        assertEquals("com.example.notes\nr1", pendingReminderOf("com.example.notes\nr1"))
        assertEquals("", pendingReminderOf(null))
        assertEquals("", pendingReminderOf("garbage"))
        val due = remindersStillDue(entries + ReminderEntry("p", "past", 10, ""), now = 100)
        assertEquals(listOf("r1"), due.map { it.id })
    }

    @Test
    fun exactAlarmsNeedTheUsersLeaveFromAndroid12() {
        assertTrue(exactAlarmsAllowed(30) { false })
        assertEquals(false, exactAlarmsAllowed(31) { false })
        assertTrue(exactAlarmsAllowed(34) { true })
    }

    // A login's redirect is only the one with our scheme; any other link is not an answer.
    @Test
    fun onlyOurSchemeEndsALogin() {
        assertTrue(isAuthRedirect("com.flickertalk.app:/oauth?code=abc", "com.flickertalk.app"))
        assertEquals(false, isAuthRedirect("https://evil.example/?code=abc", "com.flickertalk.app"))
        assertEquals(false, isAuthRedirect(null, "com.flickertalk.app"))
        assertEquals(false, isAuthRedirect("com.flickertalk.app:/x", ""))
    }

    // Native calls (2026-09-28): the router pushes `t: call` for a call, so it rings with the app
    // closed; everything else is still `wake`.
    @Test
    fun onlyTheRoutersCallPushIsACall() {
        assertEquals(true, isCall(mapOf("t" to "call", "s" to "0")))
        assertEquals(false, isCall(mapOf("t" to "wake", "s" to "0")))
        assertEquals(false, isCall(emptyMap()))
        assertEquals(false, isWake(mapOf("t" to "call")))
    }

    // The same rules as a wake-up: a closed hidden session makes no noise at all (app#9); the
    // app on the screen rings by itself; outside the weekly hours (app#7) the call shows but is
    // quiet, like a call the app rings itself.
    @Test
    fun aCallPushFollowsTheWakeUpRules() {
        val call = mapOf("t" to "call", "s" to "0")
        assertEquals(CallPush.RING, callPush(call, emptySet(), appOnScreen = false, mayDisturb = true))
        assertEquals(CallPush.SILENT, callPush(call, emptySet(), appOnScreen = false, mayDisturb = false))
        assertEquals(CallPush.IGNORE, callPush(call, emptySet(), appOnScreen = true, mayDisturb = true))
        val hidden = mapOf("t" to "call", "s" to "3")
        assertEquals(CallPush.IGNORE, callPush(hidden, emptySet(), appOnScreen = false, mayDisturb = true))
        assertEquals(CallPush.RING, callPush(hidden, setOf(3), appOnScreen = false, mayDisturb = true))
        assertEquals(CallPush.IGNORE, callPush(mapOf("t" to "wake"), emptySet(), appOnScreen = false, mayDisturb = true))
    }

    // A call rings 45 s from when the router pushed it, no longer: the caller gave up by then.
    // A phone clock that disagrees with Google's is not trusted to cut the call short.
    @Test
    fun aCallRingsFortyFiveSecondsFromItsPush() {
        assertEquals(45_000L, callRingMillis(sentAt = 1_000_000L, now = 1_000_000L))
        assertEquals(35_000L, callRingMillis(sentAt = 1_000_000L, now = 1_010_000L))
        assertEquals(45_000L, callRingMillis(sentAt = 1_000_000L, now = 990_000L), "a clock behind")
        assertEquals(45_000L, callRingMillis(sentAt = 1_000_000L, now = 1_300_000L), "a clock ahead: FCM's TTL already bounds it")
        assertEquals(45_000L, callRingMillis(sentAt = 0L, now = 1_000_000L), "no send time")
    }

    private fun assertEquals(expected: Long, actual: Long, message: String) = org.junit.Assert.assertEquals(message, expected, actual)

    // The events travel as Rust's `call_event` reads them.
    @Test
    fun callEventsTravelAsTheCoreReadsThem() {
        assertEquals(mapOf("event" to "answer"), callEventPayload(CallEvent.Answer))
        assertEquals(mapOf("event" to "end"), callEventPayload(CallEvent.End))
        assertEquals(mapOf("event" to "mute", "muted" to true), callEventPayload(CallEvent.Mute(true)))
        assertEquals(mapOf("event" to "mute", "muted" to false), callEventPayload(CallEvent.Mute(false)))
        assertEquals(mapOf("event" to "audioActivated"), callEventPayload(CallEvent.AudioActivated))
        assertEquals(mapOf("event" to "audioDeactivated"), callEventPayload(CallEvent.AudioDeactivated))
    }

    // Nothing the user does before the core listens is lost; it arrives in order when it does.
    @Test
    fun callEventsWaitUntilTheCoreListens() {
        val queue = CallEventQueue()
        val heard = mutableListOf<CallEvent>()
        queue.emit(CallEvent.Answer)
        queue.emit(CallEvent.Mute(true))
        assertEquals(emptyList<CallEvent>(), heard)
        queue.register { heard.add(it) }
        assertEquals(listOf(CallEvent.Answer, CallEvent.Mute(true)), heard)
        queue.emit(CallEvent.End)
        assertEquals(listOf(CallEvent.Answer, CallEvent.Mute(true), CallEvent.End), heard)
    }

    // A new listener replaces the old; waiting is bounded; a new call forgets an old one's events.
    @Test
    fun aNewListenerReplacesTheOldAndWaitingIsBounded() {
        val queue = CallEventQueue(limit = 2)
        val first = mutableListOf<CallEvent>()
        val second = mutableListOf<CallEvent>()
        queue.emit(CallEvent.Answer)
        queue.emit(CallEvent.Mute(true))
        queue.emit(CallEvent.End)
        queue.register { first.add(it) }
        assertEquals(listOf(CallEvent.Mute(true), CallEvent.End), first)
        queue.register { second.add(it) }
        queue.emit(CallEvent.Mute(false))
        assertEquals(listOf(CallEvent.Mute(true), CallEvent.End), first)
        assertEquals(listOf<CallEvent>(CallEvent.Mute(false)), second)
        val other = CallEventQueue()
        val none = mutableListOf<CallEvent>()
        other.emit(CallEvent.End)
        other.forget()
        other.register { none.add(it) }
        assertEquals(emptyList<CallEvent>(), none)
    }

    // Android 14 keeps the microphone from an app in the background unless a foreground service
    // of type microphone holds it, and that type needs the permission already granted.
    @Test
    fun theCallServiceHoldsTheMicrophoneOnceItIsAllowed() {
        val call = ServiceInfo.FOREGROUND_SERVICE_TYPE_PHONE_CALL
        val microphone = ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
        assertEquals(call or microphone, callServiceTypes(34, microphone = true))
        assertEquals(call, callServiceTypes(34, microphone = false))
        assertEquals(call or microphone, callServiceTypes(30, microphone = true))
        assertEquals(call, callServiceTypes(29, microphone = true), "no microphone type before Android 11")
        assertEquals(0, callServiceTypes(28, microphone = true), "no types at all before Android 10")
    }

    private fun assertEquals(expected: Int, actual: Int, message: String) = org.junit.Assert.assertEquals(message, expected.toLong(), actual.toLong())
}
