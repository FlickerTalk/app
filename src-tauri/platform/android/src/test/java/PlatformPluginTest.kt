package com.flickertalk.platform

import android.app.ActivityManager
import android.content.pm.ServiceInfo
import android.media.AudioManager
import android.view.WindowManager
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import com.android.billingclient.api.Purchase
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import java.lang.reflect.Modifier
import javax.xml.parsers.DocumentBuilderFactory

class PlatformPluginTest {
    @Test
    fun theFileProviderAuthorityBelongsToTheApp() {
        assertEquals("com.flickertalk.app.ft.files", fileProviderAuthority("com.flickertalk.app"))
    }

    @Test
    fun downloadsNeedAndroid10() {
        assertEquals(false, canSaveToDownloads(28))
        assertEquals(true, canSaveToDownloads(29))
    }

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

    @Test
    fun thePhotoPickerShowsPicturesOrVideosOrBoth() {
        assertEquals("image/*", photoPickerType("image/*"))
        assertEquals("video/*", photoPickerType("video/*"))
        assertEquals(null, photoPickerType("image/*,video/*"))
        assertEquals(null, photoPickerType(""))
    }

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

    @Test
    fun ringingFollowsTheRingerMode() {
        assertEquals(Ringing(sound = true, vibrate = true), ringingFor(AudioManager.RINGER_MODE_NORMAL))
        assertEquals(Ringing(sound = false, vibrate = true), ringingFor(AudioManager.RINGER_MODE_VIBRATE))
        assertEquals(Ringing(sound = false, vibrate = false), ringingFor(AudioManager.RINGER_MODE_SILENT))
    }

    @Test
    fun onlyWakeUpsCount() {
        assertEquals(true, isWake(mapOf("t" to "wake")))
        assertEquals(false, isWake(mapOf("t" to "other")))
        assertEquals(false, isWake(emptyMap()))
    }

    @Test
    fun notifiesOnlyWhenTheAppIsNotOnScreen() {
        assertEquals(false, shouldNotify(ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND))
        assertEquals(true, shouldNotify(ActivityManager.RunningAppProcessInfo.IMPORTANCE_CACHED))
        assertEquals(true, shouldNotify(ActivityManager.RunningAppProcessInfo.IMPORTANCE_SERVICE))
    }

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

    @Test
    fun aLanguageChangeDoesNotRecreateTheAppActivity() {
        val handled = mainActivityConfigChanges()
        assertTrue(handled.toString(), handled.containsAll(setOf("locale", "layoutDirection", "uiMode", "orientation", "screenSize")))
    }

    @Test
    fun aFontOrDisplaySizeChangeDoesNotRecreateTheAppActivity() {
        val handled = mainActivityConfigChanges()
        assertTrue(handled.toString(), handled.containsAll(setOf("fontScale", "density", "fontWeightAdjustment")))
    }

    @Test
    fun theWebViewTextZoomFollowsTheFontScale() {
        assertEquals(100, textZoomFor(1.0f))
        assertEquals(130, textZoomFor(1.3f))
        assertEquals(85, textZoomFor(0.85f))
        assertEquals(115, textZoomFor(1.15f))
        assertEquals(200, textZoomFor(2.0f))
    }

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

    @Test
    fun callsHaveTheirOwnNotificationChannel() {
        assertEquals("ft.call", CALL_CHANNEL)
    }

    @Test
    fun theCallNotificationSaysWhoIsCallingAndWhatKind() {
        assertEquals("Alice", callTitle("Alice"))
        assertNull(callTitle("  "))
        assertEquals(R.string.ft_incoming_video_call, callText(true))
        assertEquals(R.string.ft_incoming_call, callText(false))
    }

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

    @Test
    fun notificationTextsAreTranslatedInEveryLanguage() {
        val sameAsEnglish = mapOf(
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

    @Test
    fun aQuietCallNeitherRingsNorVibrates() {
        assertEquals(Ringing(sound = false, vibrate = false), ringingFor(AudioManager.RINGER_MODE_NORMAL, quiet = true))
        assertEquals(Ringing(sound = true, vibrate = true), ringingFor(AudioManager.RINGER_MODE_NORMAL, quiet = false))
    }

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

    @Test
    fun theOpenSlotsAreKeptForAProcessAPushStarts() {
        assertEquals(setOf(1, 3, 7), slotsKept(keptSlots(setOf(3, 1, 7))))
        assertEquals(emptySet<Int>(), slotsKept(keptSlots(emptySet())))
        assertEquals("nothing kept: none open", emptySet<Int>(), slotsKept(null))
        assertEquals(setOf(2), slotsKept("2,0,8,x,,-1"))
    }

    private val NOW = 1_850_000_000_000L

    private fun purchase(
        product: String = YEARLY,
        state: Int = Purchase.PurchaseState.PURCHASED,
        boughtAt: Long = 1790208000000L,
        acknowledged: Boolean = true,
    ) = StorePurchase(product, state, boughtAt, acknowledged)

    @Test
    fun aListedSubscriptionIsKeptForAYearFromTheCheckHoweverOldItsPurchase() {
        val now = 1_850_000_000_000L
        val year = 365L * 24 * 60 * 60 * 1000
        val boughtTwoYearsAgo = now - 2 * year
        assertEquals(now + year, activeUntil(listOf(purchase(boughtAt = boughtTwoYearsAgo)), now))
        assertEquals(now + 60_000 + year, activeUntil(listOf(purchase(boughtAt = boughtTwoYearsAgo)), now + 60_000))
    }

    @Test
    fun aPendingPurchaseBuysNothingYet() {
        assertEquals(0L, activeUntil(listOf(purchase(state = Purchase.PurchaseState.PENDING)), NOW))
    }

    @Test
    fun nothingBoughtIsNoSubscription() {
        assertEquals(0L, activeUntil(emptyList(), NOW))
    }

    @Test
    fun onlyTheYearlyProductCounts() {
        assertEquals(0L, activeUntil(listOf(purchase(product = "com.someone.else.pro")), NOW))
    }

    @Test
    fun aPurchaseThatOnlyWaitsSaysItIsPending() {
        assertEquals("pending_approval", purchaseTrouble(listOf(purchase(state = Purchase.PurchaseState.PENDING))))
        assertNull(purchaseTrouble(listOf(purchase())))
        assertNull(purchaseTrouble(emptyList()))
        assertNull(purchaseTrouble(listOf(purchase(state = Purchase.PurchaseState.PENDING), purchase())))
    }

    @Test
    fun aPaidPurchaseIsAcknowledgedOnce() {
        assertEquals(true, needsAcknowledgement(purchase(acknowledged = false)))
        assertEquals(false, needsAcknowledgement(purchase(acknowledged = true)))
        assertEquals(
            false,
            needsAcknowledgement(purchase(state = Purchase.PurchaseState.PENDING, acknowledged = false)),
        )
    }

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
        val later = mutableListOf<Long>()
        queue.register { later.add(it) }
        assertEquals(emptyList<Long>(), later)
        queue.offer(5L)
        assertEquals(listOf(0L, 1_830_000_000_000L), heard)
        assertEquals(listOf(5L), later)
    }

    private fun offer(
        basePlan: String = YEARLY_BASE_PLAN,
        offerId: String? = null,
        token: String = "base-token",
        prices: List<String> = listOf("0,99 €"),
    ) = StoreOffer(basePlan, offerId, token, prices)

    @Test
    fun theYearlyPriceIsTheBasePlansAsPlayFormatsIt() {
        assertEquals("0,99 €", yearlyPrice(listOf(offer())))
        assertEquals("US$0.99", yearlyPrice(listOf(offer(prices = listOf("US$0.99")))))
    }

    @Test
    fun aPromotionDoesNotChangeThePriceShown() {
        val promotion = offer(offerId = "intro", token = "intro-token", prices = listOf("0,49 €", "0,99 €"))
        val offers = listOf(promotion, offer())
        assertEquals("0,99 €", yearlyPrice(offers))
        assertEquals("base-token", yearlyOffer(offers)?.token)
    }

    @Test
    fun withOnlyAnOfferTheRenewingPriceCounts() {
        assertEquals("0,99 €", yearlyPrice(listOf(offer(offerId = "intro", prices = listOf("Free", "0,99 €")))))
    }

    @Test
    fun withoutTheYearlyPlanThereIsNoPrice() {
        assertNull(yearlyPrice(emptyList()))
        assertNull(yearlyPrice(listOf(offer(basePlan = "monthly-autorenew"))))
        assertNull(yearlyPrice(listOf(offer(prices = emptyList()))))
        assertNull(yearlyPrice(listOf(offer(prices = listOf("  ")))))
        assertNull(yearlyOffer(listOf(offer(basePlan = "monthly-autorenew"))))
    }

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

    @Test
    fun theExactAlarmScreenOpensOnlyWhereTheyAreNotAllowed() {
        assertFalse(opensExactAlarmSettings(30, allowed = false))
        assertTrue(opensExactAlarmSettings(31, allowed = false))
        assertFalse(opensExactAlarmSettings(31, allowed = true))
        assertTrue(opensExactAlarmSettings(34, allowed = false))
    }

    @Test
    fun remindersAreSetAgainAfterABootAndOnceExactAlarmsAreAllowed() {
        assertTrue(setsRemindersAgain("android.intent.action.BOOT_COMPLETED"))
        assertTrue(setsRemindersAgain("android.app.action.SCHEDULE_EXACT_ALARM_PERMISSION_STATE_CHANGED"))
        assertFalse(setsRemindersAgain("android.intent.action.MY_PACKAGE_REPLACED"))
        assertFalse(setsRemindersAgain(null))
    }

    @Test
    fun theManifestSetsRemindersAgainOnceExactAlarmsAreAllowed() {
        val manifest = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(File("src/main/AndroidManifest.xml"))
        fun elements(tag: String) = manifest.getElementsByTagName(tag).let { nodes -> (0 until nodes.length).map { nodes.item(it) as Element } }
        val permissions = elements("uses-permission").map { it.getAttribute("android:name") }
        assertTrue(permissions.contains("android.permission.SCHEDULE_EXACT_ALARM"))
        assertFalse(permissions.contains("android.permission.USE_EXACT_ALARM"))
        val receiver = elements("receiver").single { it.getAttribute("android:name") == "com.flickertalk.platform.BootReceiver" }
        val actions = receiver.getElementsByTagName("action").let { nodes -> (0 until nodes.length).map { (nodes.item(it) as Element).getAttribute("android:name") } }
        assertEquals(
            listOf("android.intent.action.BOOT_COMPLETED", "android.app.action.SCHEDULE_EXACT_ALARM_PERMISSION_STATE_CHANGED"),
            actions,
        )
    }

    @Test
    fun onlyOurSchemeEndsALogin() {
        assertTrue(isAuthRedirect("com.flickertalk.app:/oauth?code=abc", "com.flickertalk.app"))
        assertEquals(false, isAuthRedirect("https://evil.example/?code=abc", "com.flickertalk.app"))
        assertEquals(false, isAuthRedirect(null, "com.flickertalk.app"))
        assertEquals(false, isAuthRedirect("com.flickertalk.app:/x", ""))
    }

    @Test
    fun onlyTheRoutersCallPushIsACall() {
        assertEquals(true, isCall(mapOf("t" to "call", "s" to "0")))
        assertEquals(false, isCall(mapOf("t" to "wake", "s" to "0")))
        assertEquals(false, isCall(emptyMap()))
        assertEquals(false, isWake(mapOf("t" to "call")))
    }

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

    @Test
    fun aCallRingsFortyFiveSecondsFromItsPush() {
        assertEquals(45_000L, callRingMillis(sentAt = 1_000_000L, now = 1_000_000L))
        assertEquals(35_000L, callRingMillis(sentAt = 1_000_000L, now = 1_010_000L))
        assertEquals(45_000L, callRingMillis(sentAt = 1_000_000L, now = 990_000L), "a clock behind")
        assertEquals(45_000L, callRingMillis(sentAt = 1_000_000L, now = 1_300_000L), "a clock ahead: FCM's TTL already bounds it")
        assertEquals(45_000L, callRingMillis(sentAt = 0L, now = 1_000_000L), "no send time")
    }

    private fun assertEquals(expected: Long, actual: Long, message: String) = org.junit.Assert.assertEquals(message, expected, actual)

    @Test
    fun callEventsTravelAsTheCoreReadsThem() {
        assertEquals(mapOf("event" to "answer"), callEventPayload(CallEvent.Answer))
        assertEquals(mapOf("event" to "end"), callEventPayload(CallEvent.End))
        assertEquals(mapOf("event" to "mute", "muted" to true), callEventPayload(CallEvent.Mute(true)))
        assertEquals(mapOf("event" to "mute", "muted" to false), callEventPayload(CallEvent.Mute(false)))
        assertEquals(mapOf("event" to "audioActivated"), callEventPayload(CallEvent.AudioActivated))
        assertEquals(mapOf("event" to "audioDeactivated"), callEventPayload(CallEvent.AudioDeactivated))
    }

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

    @Test
    fun theAudioModeGoesBackAfterACall() {
        assertEquals(AudioManager.MODE_NORMAL, modeAfterCall(AudioManager.MODE_NORMAL))
        assertEquals(AudioManager.MODE_RINGTONE, modeAfterCall(AudioManager.MODE_RINGTONE))
        assertEquals(AudioManager.MODE_NORMAL, modeAfterCall(AudioManager.MODE_IN_COMMUNICATION))
        assertEquals(AudioManager.MODE_NORMAL, modeAfterCall(AudioManager.MODE_IN_CALL))
        assertEquals(AudioManager.MODE_NORMAL, modeAfterCall(null))
    }

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

    @Test
    fun aCallShowsOverTheLockScreenWhileItRingsOrGoesOn() {
        assertTrue(overLockScreen(ringing = true, inCall = false))
        assertTrue(overLockScreen(ringing = false, inCall = true))
        assertFalse(overLockScreen(ringing = false, inCall = false))
    }

    @Test
    fun theScreenStaysOnWhileACallGoesOn() {
        val added = mutableListOf<Int>()
        val cleared = mutableListOf<Int>()
        keepScreenOnFlags(inCall = true, add = { added += it }, clear = { cleared += it })
        assertEquals(listOf(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON), added)
        assertEquals(emptyList<Int>(), cleared)
        keepScreenOnFlags(inCall = false, add = { added += it }, clear = { cleared += it })
        assertEquals("nothing more is added", listOf(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON), added)
        assertEquals(listOf(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON), cleared)
    }

    @Test
    fun theMicrophoneTypeNeedsTheAppOnScreenOrTheUsersAnswer() {
        assertTrue(microphoneServiceAllowed(granted = true, visible = true, answeredByTap = false))
        assertTrue(microphoneServiceAllowed(granted = true, visible = false, answeredByTap = true))
        assertFalse(microphoneServiceAllowed(granted = true, visible = false, answeredByTap = false))
        assertFalse(microphoneServiceAllowed(granted = false, visible = true, answeredByTap = true))
    }

    @Test
    fun aCallPushedWithTheAppClosedRingsOnItsOwnChannel() {
        assertEquals("ft.call.ringing", RINGING_CALL_CHANNEL)
    }

    @Test
    fun aRefusedCallCancelsOnlyThePushsRinging() {
        assertTrue(refusalCancels(coreRinging = false, inCall = false))
        assertFalse(refusalCancels(coreRinging = true, inCall = false))
        assertFalse(refusalCancels(coreRinging = false, inCall = true))
    }

    private val ringingCall = RingingCall(title = "Ana", text = "Incoming call", video = false)

    private class FakeCallNotifications {
        val posted = mutableListOf<RingingCall>()
        var cancels = 0

        fun notice() = RingingNotice(post = { posted.add(it) }, cancel = { cancels += 1 })
    }

    @Test
    fun aCallTheCoreRingsWithTheAppOnScreenPostsNoNotification() {
        val notifications = FakeCallNotifications()
        notifications.notice().ring(ringingCall, appVisible = true)
        assertEquals(emptyList<RingingCall>(), notifications.posted)
        assertEquals(1, notifications.cancels)
    }

    @Test
    fun aCallTheCoreRingsWithTheAppOffScreenPostsItsNotificationAsBefore() {
        val notifications = FakeCallNotifications()
        val notice = notifications.notice()
        notice.ring(ringingCall, appVisible = false)
        assertEquals(listOf(ringingCall), notifications.posted)
        notice.appHidden()
        assertEquals("posted once", listOf(ringingCall), notifications.posted)
    }

    @Test
    fun aCallStillRingingWhenTheAppLeavesTheScreenPostsItsNotificationThen() {
        val notifications = FakeCallNotifications()
        val notice = notifications.notice()
        notice.ring(ringingCall, appVisible = true)
        assertEquals("nothing while the app is on the screen", emptyList<RingingCall>(), notifications.posted)
        notice.appHidden()
        assertEquals(listOf(ringingCall), notifications.posted)
        notice.appHidden()
        assertEquals("once per call, however often the app leaves the screen", listOf(ringingCall), notifications.posted)
    }

    @Test
    fun stoppingTheRingingCancelsTheNotificationOnOrOffScreen() {
        for (visible in listOf(true, false)) {
            val notifications = FakeCallNotifications()
            val notice = notifications.notice()
            notice.ring(ringingCall, appVisible = visible)
            val shown = notifications.posted.toList()
            val cancels = notifications.cancels
            notice.stop()
            assertEquals("cancelled, app on screen: $visible", cancels + 1L, notifications.cancels.toLong())
            notice.appHidden()
            assertEquals("a call that stopped ringing never shows, app on screen: $visible", shown, notifications.posted)
        }
    }

    @Test
    fun theSystemBarsFollowTheAppsAppearance() {
        assertEquals(SystemBarsLook(darkIcons = false, navigationBarColor = 0), systemBarsLook(dark = true, sdk = 33))
        assertEquals(SystemBarsLook(darkIcons = true, navigationBarColor = 0), systemBarsLook(dark = false, sdk = 33))
        assertEquals(SystemBarsLook(darkIcons = true, navigationBarColor = 0), systemBarsLook(dark = false, sdk = 26))
        assertEquals(SystemBarsLook(darkIcons = true, navigationBarColor = NAVIGATION_SCRIM), systemBarsLook(dark = false, sdk = 25))
        assertEquals(SystemBarsLook(darkIcons = false, navigationBarColor = 0), systemBarsLook(dark = true, sdk = 24))
        assertEquals(0x801B1B1B.toInt(), NAVIGATION_SCRIM)
    }

    @Test
    fun thePluginKeepsEveryCommandRustCalls() {
        val expected = setOf(
            "pendingReminder", "openedLink", "registerLinkEvents", "setReminders", "exactAlarms", "requestExactAlarms",
            "authorize", "callAnswering", "registerCallEvents", "callStartedOutgoing", "callConnected", "callEnded",
            "setSystemBars", "setAppearance", "setSpeaker", "requestMicrophone", "currentLocation", "translateLanguages", "translateDetect",
            "translateStatus", "translatePrepare", "translateRemove", "translateText", "requestCamera", "attachVideo",
            "videoLayout", "videoShape", "detachVideo", "callVideo", "canShowFullScreen", "askFullScreen", "startRinging",
            "sealKey", "forgetKey", "openKey", "pushToken", "requestNotifications", "restartApp", "setOpenSlots",
            "setQuietHours", "callRefused", "stopRinging", "shareText", "pickFiles", "takePhoto", "openFile", "shareFile",
            "subscriptionPrice", "subscribe", "subscription", "registerEntitlementEvents", "restoreSubscription",
            "saveToDownloads", "printFile",
        )
        val commands = PlatformPlugin::class.java.declaredMethods
            .filter { it.isAnnotationPresent(Command::class.java) }
        assertTrue(commands.all { it.parameterTypes.toList() == listOf(Invoke::class.java) && Modifier.isPublic(it.modifiers) })
        assertEquals(expected, commands.map { it.name }.toSet())
        val rust = File("../src/lib.rs").readText()
        for (name in expected) assertTrue(name, rust.contains("\"$name\""))
    }

    @Test
    fun thePluginKeepsItsCallbacksAndPermissionAliases() {
        fun annotated(type: Class<out Annotation>) =
            PlatformPlugin::class.java.declaredMethods.filter { it.isAnnotationPresent(type) }.map { it.name }.toSet()
        assertEquals(setOf("microphoneAnswered", "locationAnswered", "cameraAnswered"), annotated(PermissionCallback::class.java))
        assertEquals(setOf("photoTaken", "picked"), annotated(ActivityCallback::class.java))
        val aliases = PlatformPlugin::class.java.getAnnotation(TauriPlugin::class.java)!!.permissions
            .associate { it.alias to it.strings.toList() }
        assertEquals(
            mapOf(
                "microphone" to listOf("android.permission.RECORD_AUDIO"),
                "camera" to listOf("android.permission.CAMERA"),
                "location" to listOf("android.permission.ACCESS_COARSE_LOCATION", "android.permission.ACCESS_FINE_LOCATION"),
            ),
            aliases,
        )
    }

    @Test
    fun theCommandArgumentsKeepTheFieldsRustSends() {
        fun fields(type: Class<*>) = type.declaredFields.map { it.name }.toSet()
        assertEquals(setOf("reminders"), fields(RemindersArgs::class.java))
        assertEquals(setOf("url", "scheme"), fields(AuthorizeArgs::class.java))
        assertEquals(setOf("value"), fields(KeyArgs::class.java))
        assertEquals(setOf("caller", "video", "muted"), fields(RingingArgs::class.java))
        assertEquals(setOf("caller", "video"), fields(AnsweringArgs::class.java))
        assertEquals(setOf("channel"), fields(CallEventsArgs::class.java))
        assertEquals(setOf("name", "video"), fields(OutgoingArgs::class.java))
        assertEquals(setOf("dark"), fields(SystemBarsArgs::class.java))
        assertEquals(setOf("appearance"), fields(AppearanceArgs::class.java))
        assertEquals(setOf("on"), fields(SpeakerArgs::class.java))
        assertEquals(setOf("week"), fields(QuietHoursArgs::class.java))
        assertEquals(setOf("slots"), fields(OpenSlotsArgs::class.java))
        assertEquals(setOf("text"), fields(ShareTextArgs::class.java))
        assertEquals(setOf("accept"), fields(PickArgs::class.java))
        assertEquals(setOf("path", "mime"), fields(OpenFileArgs::class.java))
        assertEquals(setOf("path", "name", "mime"), fields(SaveFileArgs::class.java))
    }

    private fun mainActivityConfigChanges(): Set<String> {
        val nodes = DocumentBuilderFactory.newInstance().newDocumentBuilder()
            .parse(File("../../gen/android/app/src/main/AndroidManifest.xml")).getElementsByTagName("activity")
        val main = (0 until nodes.length).map { nodes.item(it) as Element }
            .first { it.getAttribute("android:name") == ".MainActivity" }
        return main.getAttribute("android:configChanges").split("|").toSet()
    }
}
