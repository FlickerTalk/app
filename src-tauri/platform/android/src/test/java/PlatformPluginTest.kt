package com.flickertalk.platform

import android.app.ActivityManager
import android.content.pm.ServiceInfo
import android.media.AudioManager
import com.android.billingclient.api.Purchase
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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

    // app#70 (2026-10-03): the system prompt hides the page, and closing it makes the page
    // visible again, which hands the push token over once more. Asking again then showed the
    // prompt a second time, whose refusal is final. Once per process start, only where needed.
    @Test
    fun asksForNotificationsAtMostOncePerProcess() {
        val prompt = NotificationPrompt()
        assertTrue(prompt.shouldAsk(33, granted = false))
        assertFalse(prompt.shouldAsk(33, granted = false))
        assertFalse(prompt.shouldAsk(36, granted = false))
    }

    @Test
    fun neverAsksForNotificationsBeforeAndroid13OrOnceGranted() {
        assertFalse(NotificationPrompt().shouldAsk(32, granted = false))
        assertFalse(NotificationPrompt().shouldAsk(33, granted = true))
        val prompt = NotificationPrompt()
        assertFalse(prompt.shouldAsk(33, granted = true))
        assertTrue(prompt.shouldAsk(33, granted = false))
    }

    // app#82 (2026-10-03): a recreated activity gets a new WebView, but the plugins keep the first
    // activity and its WebView, so the old page lived on with the bridge. A change of language
    // (Android flags locale and layoutDirection together) must not recreate the app's activity.
    @Test
    fun aLanguageChangeDoesNotRecreateTheAppActivity() {
        val handled = mainActivityConfigChanges()
        assertTrue(handled.toString(), handled.containsAll(setOf("locale", "layoutDirection", "uiMode", "orientation", "screenSize")))
    }

    // app#92 (2026-10-03): the same leak followed a change of font size, display size or bold
    // text. None of them may recreate the app's activity; the WebView follows them in place.
    @Test
    fun aFontOrDisplaySizeChangeDoesNotRecreateTheAppActivity() {
        val handled = mainActivityConfigChanges()
        assertTrue(handled.toString(), handled.containsAll(setOf("fontScale", "density", "fontWeightAdjustment")))
    }

    // Without a recreation the WebView keeps the text zoom it took from the font scale when it
    // was created (Chromium's AwSettings), so a new font scale has to reach it as its text zoom.
    @Test
    fun theWebViewTextZoomFollowsTheFontScale() {
        assertEquals(100, textZoomFor(1.0f))
        assertEquals(130, textZoomFor(1.3f))
        assertEquals(85, textZoomFor(0.85f))
        assertEquals(115, textZoomFor(1.15f))
        assertEquals(200, textZoomFor(2.0f))
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
        val english = notificationTexts("values")
        assertTrue(english.isNotEmpty())
        for (language in notificationLanguages) {
            val translated = notificationTexts("values-$language")
            assertEquals(language, english.keys, translated.keys)
            translated.forEach { (name, text) -> assertTrue("$language/$name", text.isNotEmpty()) }
        }
    }

    // A text identical to the English one is a text nobody translated, unless it is listed here
    // with the reason it may stay as it is.
    @Test
    fun notificationTextsAreTranslatedInEveryLanguage() {
        val sameAsEnglish = mapOf(
            // "Video" is the word in these languages too.
            "ft_video" to setOf("de", "it", "in", "ro", "tr", "vi"),
        )
        val english = notificationTexts("values")
        val untranslated = notificationLanguages.associateWith { language ->
            notificationTexts("values-$language")
                .filter { (name, text) -> text == english[name] && language !in sameAsEnglish[name].orEmpty() }
                .keys.sorted()
        }.filterValues { it.isNotEmpty() }
        assertEquals("texts still in English", emptyMap<String, List<String>>(), untranslated)
    }

    private val notificationLanguages = listOf(
        "es", "pt", "fr", "de", "it", "ro", "ru", "uk", "pl", "tr", "ar",
        "hi", "bn", "in", "vi", "th", "ja", "ko", "zh-rCN", "zh-rTW",
    )

    private fun notificationTexts(dir: String): Map<String, String> {
        val file = File("src/main/res/$dir/ft_strings.xml")
        assertTrue("missing $file", file.isFile)
        val nodes = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(file)
            .getElementsByTagName("string")
        return (0 until nodes.length).map { nodes.item(it) as Element }
            .associate { it.getAttribute("name") to it.textContent.trim() }
    }

    // Bug of 2026-09-29 (QA, emulators): with the app closed, "Answer" on the call notification
    // opened the app, and the WebView read the answer before the offer had arrived and dropped it;
    // the phone rang again. The notification's buttons now go to the core, as CallKit's do, through
    // the call events channel, where they wait until the core listens: the core keeps an early
    // answer (or decline) for the offer still on its way.
    @Test
    fun theNotificationsButtonsGoToTheCore() {
        assertEquals(CallEvent.Answer, tapCallEvent(NotificationTap.CALL_ANSWER))
        assertEquals(CallEvent.Decline, tapCallEvent(NotificationTap.CALL_DECLINE))
        assertEquals(mapOf("event" to "decline"), callEventPayload(CallEvent.Decline))
        val queue = CallEventQueue()
        val heard = mutableListOf<CallEvent>()
        queue.emit(CallEvent.Answer)
        queue.register { heard.add(it) }
        assertEquals(listOf<CallEvent>(CallEvent.Answer), heard)
    }

    // The order seen on the Lenovo tablet (2026-09-29, app closed): FCM started the process and
    // rang, the user tapped "Answer" on the notification before Rust listened, and the offer came
    // seconds later. The tap must reach the core once it registers, whatever came before it in the
    // new process: the push's `Incoming` and the activity's `Visible` go only to a core that
    // listens (nothing of them waits), the answer waits and is heard once, in order.
    @Test
    fun theNotificationsAnswerTappedAsTheProcessStartsReachesTheCoreWhenItListens() {
        val queue = CallEventQueue()
        assertFalse("FCM's wake-up waits for no one", queue.offer(CallEvent.Incoming))
        assertFalse("the activity's visibility waits for no one", queue.offer(CallEvent.Visible(true)))
        queue.emit(tapCallEvent(NotificationTap.CALL_ANSWER) ?: error("the notification's answer is an event"))
        val heard = mutableListOf<CallEvent>()
        queue.register { heard.add(it) }
        assertEquals(listOf<CallEvent>(CallEvent.Answer), heard)
        queue.register { heard.add(it) }
        assertEquals("heard once", listOf<CallEvent>(CallEvent.Answer), heard)
        queue.emit(CallEvent.Mute(true))
        assertEquals(listOf(CallEvent.Answer, CallEvent.Mute(true)), heard)
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

    // 2026-10-02: each copy the picker makes is a new file of its own, named as on iOS. Two files
    // with the same name in one pick, or a clock set back, never write over one a message points to.
    @Test
    fun aPickedCopyNeverTakesTheNameOfAFileThatIsThere() {
        val folder = java.nio.file.Files.createTempDirectory("ft-picked").toFile()
        assertEquals("1759400000000-0-a.jpg", pickedBase(1759400000000, 0, "a.jpg"))
        assertEquals("a name never makes a path", "1-0-a_b.pdf", pickedBase(1, 0, "a/b.pdf"))

        val first = newPickedFile(folder, pickedBase(1759400000000, 0, "a.jpg"))
        val second = newPickedFile(folder, pickedBase(1759400000000, 1, "a.jpg"))
        assertEquals("1759400000000-1-a.jpg", second.name)
        first.writeText("sent")
        val again = newPickedFile(folder, pickedBase(1759400000000, 0, "a.jpg"))
        assertEquals("1759400000000-0-a-2.jpg", again.name)
        assertEquals("sent", first.readText())
        assertTrue("it is ours from the start", again.exists())
        folder.deleteRecursively()
    }

    // The camera names a photo by the second it was taken: a second one in that second is its own file.
    @Test
    fun twoPhotosInOneSecondAreTwoFiles() {
        val folder = java.nio.file.Files.createTempDirectory("ft-photo").toFile()
        val name = photoName(0, java.util.TimeZone.getTimeZone("UTC"))
        val first = newPickedFile(folder, name)
        first.writeText("first")
        val second = newPickedFile(folder, name)
        assertEquals("photo-19700101-000000-2.jpg", second.name)
        assertEquals("first", first.readText())
        val third = newPickedFile(folder, name)
        assertEquals("photo-19700101-000000-3.jpg", third.name)
        folder.deleteRecursively()
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

    // 2026-10-01 (§108): a session stays open until the user leaves it, across starts. The open
    // slots are kept in preferences, so a process a push starts, before the core, still hears
    // them. Only slots 1–7 are read back; anything else is not a slot.
    @Test
    fun theOpenSlotsAreKeptForAProcessAPushStarts() {
        assertEquals(setOf(1, 3, 7), slotsKept(keptSlots(setOf(3, 1, 7))))
        assertEquals(emptySet<Int>(), slotsKept(keptSlots(emptySet())))
        assertEquals("nothing kept: none open", emptySet<Int>(), slotsKept(null))
        assertEquals(setOf(2), slotsKept("2,0,8,x,,-1"))
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

    // 2026-10-07: what Play says with the app open (back on the screen, a purchase that was pending)
    // waits for the core if it is not listening yet. Only the latest counts: an older word from the
    // Store is no longer true.
    @Test
    fun aLiveEntitlementWaitsForTheCoreAndOnlyTheLatestCounts() {
        val queue = EntitlementQueue()
        queue.offer(1_800_000_000_000L)
        queue.offer(0L)
        val heard = mutableListOf<Long>()
        queue.register { heard.add(it) }
        assertEquals(listOf(0L), heard)
        queue.offer(1_830_000_000_000L)
        assertEquals(listOf(0L, 1_830_000_000_000L), heard)
        // A new core (the phone was erased) hears from now on, and nothing is told twice.
        val later = mutableListOf<Long>()
        queue.register { later.add(it) }
        assertEquals(emptyList<Long>(), later)
        queue.offer(5L)
        assertEquals(listOf(0L, 1_830_000_000_000L), heard)
        assertEquals(listOf(5L), later)
    }

    // One offer of the subscription as Play describes it: its base plan, its own id (none for the
    // base plan itself), its token and the price of each phase, as Play formats it.
    private fun offer(
        basePlan: String = YEARLY_BASE_PLAN,
        offerId: String? = null,
        token: String = "base-token",
        prices: List<String> = listOf("0,99 €"),
    ) = StoreOffer(basePlan, offerId, token, prices)

    // The Plan screen shows the price as the Store formats it for this phone (currency, commas,
    // taxes): nothing is converted or rounded here (2026-09-29).
    @Test
    fun theYearlyPriceIsTheBasePlansAsPlayFormatsIt() {
        assertEquals("0,99 €", yearlyPrice(listOf(offer())))
        assertEquals("US$0.99", yearlyPrice(listOf(offer(prices = listOf("US$0.99")))))
    }

    // An introductory offer is not what a year costs: the base plan's price is shown, and the
    // purchase goes through that same base plan.
    @Test
    fun aPromotionDoesNotChangeThePriceShown() {
        val promotion = offer(offerId = "intro", token = "intro-token", prices = listOf("0,49 €", "0,99 €"))
        val offers = listOf(promotion, offer())
        assertEquals("0,99 €", yearlyPrice(offers))
        assertEquals("base-token", yearlyOffer(offers)?.token)
    }

    // With only an offer of the yearly plan, its last phase is the price that keeps renewing.
    @Test
    fun withOnlyAnOfferTheRenewingPriceCounts() {
        assertEquals("0,99 €", yearlyPrice(listOf(offer(offerId = "intro", prices = listOf("Free", "0,99 €")))))
    }

    // When the Store cannot say (no product, another plan, an empty price) there is no price, and
    // the screen says "yearly subscription" without any amount.
    @Test
    fun withoutTheYearlyPlanThereIsNoPrice() {
        assertNull(yearlyPrice(emptyList()))
        assertNull(yearlyPrice(listOf(offer(basePlan = "monthly-autorenew"))))
        assertNull(yearlyPrice(listOf(offer(prices = emptyList()))))
        assertNull(yearlyPrice(listOf(offer(prices = listOf("  ")))))
        assertNull(yearlyOffer(listOf(offer(basePlan = "monthly-autorenew"))))
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

    // After a call the phone's audio goes back to how it was, never stuck in a call's mode.
    @Test
    fun theAudioModeGoesBackAfterACall() {
        assertEquals(AudioManager.MODE_NORMAL, modeAfterCall(AudioManager.MODE_NORMAL))
        assertEquals(AudioManager.MODE_RINGTONE, modeAfterCall(AudioManager.MODE_RINGTONE))
        assertEquals(AudioManager.MODE_NORMAL, modeAfterCall(AudioManager.MODE_IN_COMMUNICATION))
        assertEquals(AudioManager.MODE_NORMAL, modeAfterCall(AudioManager.MODE_IN_CALL))
        assertEquals(AudioManager.MODE_NORMAL, modeAfterCall(null))
    }

    // Bug of 2026-09-28: a phone asleep had lost its socket to the router, and only the WebView
    // asked to reconnect. A call or wake-up push tells a core that listens to reconnect at once;
    // a process FCM just started has no core yet, and it connects when it starts.
    @Test
    fun aCallOrWakePushTellsALiveCoreToReconnect() {
        assertEquals(mapOf("event" to "incoming"), callEventPayload(CallEvent.Incoming))
        assertTrue(reconnectsOnPush(mapOf("t" to "call", "s" to "0"), emptySet()))
        assertTrue(reconnectsOnPush(mapOf("t" to "wake"), emptySet()))
        assertFalse("a closed hidden session stays quiet", reconnectsOnPush(mapOf("t" to "call", "s" to "3"), emptySet()))
        assertFalse(reconnectsOnPush(mapOf("t" to "other"), emptySet()))
        val queue = CallEventQueue()
        val heard = mutableListOf<CallEvent>()
        assertFalse(queue.offer(CallEvent.Incoming))
        queue.register { heard.add(it) }
        assertEquals(emptyList<CallEvent>(), heard)
        assertTrue(queue.offer(CallEvent.Incoming))
        assertEquals(listOf<CallEvent>(CallEvent.Incoming), heard)
    }

    // Speaker or receiver (2026-09-28): Android 12 chooses the communication device; before, the
    // speakerphone switch.
    @Test
    fun theSpeakerIsTheCommunicationDeviceFromAndroid12() {
        assertEquals(SpeakerRoute.SPEAKER_DEVICE, speakerRoute(31, on = true))
        assertEquals(SpeakerRoute.CLEAR_DEVICE, speakerRoute(34, on = false))
        assertEquals(SpeakerRoute.SPEAKERPHONE_ON, speakerRoute(30, on = true))
        assertEquals(SpeakerRoute.SPEAKERPHONE_OFF, speakerRoute(30, on = false))
    }

    // A call shows over the lock screen and turns the screen on while it rings or goes on.
    @Test
    fun aCallShowsOverTheLockScreenWhileItRingsOrGoesOn() {
        assertTrue(overLockScreen(ringing = true, inCall = false))
        assertTrue(overLockScreen(ringing = false, inCall = true))
        assertFalse(overLockScreen(ringing = false, inCall = false))
    }

    // Android 14 lets a foreground service take the microphone only while the app is on the
    // screen, or right after the user's own answer.
    @Test
    fun theMicrophoneTypeNeedsTheAppOnScreenOrTheUsersAnswer() {
        assertTrue(microphoneServiceAllowed(granted = true, visible = true, answeredByTap = false))
        assertTrue(microphoneServiceAllowed(granted = true, visible = false, answeredByTap = true))
        assertFalse(microphoneServiceAllowed(granted = true, visible = false, answeredByTap = false))
        assertFalse(microphoneServiceAllowed(granted = false, visible = true, answeredByTap = true))
    }

    // With the app closed the system rings (the process may be frozen), on a channel of its own:
    // the in-app one is silent because the app plays the ringtone itself.
    @Test
    fun aCallPushedWithTheAppClosedRingsOnItsOwnChannel() {
        assertEquals("ft.call.ringing", RINGING_CALL_CHANNEL)
    }

    // 2026-10-01: the core refused a call without a trace (Calls off, a stranger, a blocked
    // contact). The ringing a call push started before the core knew who called ends at once; a
    // call the core rings itself, or one going on, is another call and is left alone.
    @Test
    fun aRefusedCallCancelsOnlyThePushsRinging() {
        assertTrue(refusalCancels(coreRinging = false, inCall = false))
        assertFalse(refusalCancels(coreRinging = true, inCall = false))
        assertFalse(refusalCancels(coreRinging = false, inCall = true))
    }

    // 2026-10-02: the system bars' icons follow the app's appearance, not the system's: light on
    // the dark app, dark on the light one. Before Android 8 the navigation bar's icons cannot be
    // dark, so on the light app the bar gets a dark scrim behind its light ones; otherwise it
    // stays see-through over the app's own strip.
    @Test
    fun theSystemBarsFollowTheAppsAppearance() {
        assertEquals(SystemBarsLook(darkIcons = false, navigationBarColor = 0), systemBarsLook(dark = true, sdk = 33))
        assertEquals(SystemBarsLook(darkIcons = true, navigationBarColor = 0), systemBarsLook(dark = false, sdk = 33))
        assertEquals(SystemBarsLook(darkIcons = true, navigationBarColor = 0), systemBarsLook(dark = false, sdk = 26))
        assertEquals(SystemBarsLook(darkIcons = true, navigationBarColor = NAVIGATION_SCRIM), systemBarsLook(dark = false, sdk = 25))
        assertEquals(SystemBarsLook(darkIcons = false, navigationBarColor = 0), systemBarsLook(dark = true, sdk = 24))
        // androidx's own dark scrim (`SystemBarStyle.auto`), half-transparent.
        assertEquals(0x801B1B1B.toInt(), NAVIGATION_SCRIM)
    }

    // The configuration changes the app's MainActivity handles itself, from its manifest.
    private fun mainActivityConfigChanges(): Set<String> {
        val nodes = DocumentBuilderFactory.newInstance().newDocumentBuilder()
            .parse(File("../../gen/android/app/src/main/AndroidManifest.xml")).getElementsByTagName("activity")
        val main = (0 until nodes.length).map { nodes.item(it) as Element }
            .first { it.getAttribute("android:name") == ".MainActivity" }
        return main.getAttribute("android:configChanges").split("|").toSet()
    }
}
