package com.flickertalk.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import app.tauri.annotation.Command
import app.tauri.plugin.Invoke
import java.io.File

// On-device translation (2026-10-20, app#177): ML Kit translates on the phone, for a plugin the
// user granted `translate`. The text never leaves the phone; a language's model is downloaded
// once, only after the user agreed to it.
class TranslateTest {
    // The bridge answers only for the tags it lists; a region, an unknown code or nothing is not
    // one of them, so it is unsupported rather than an error.
    @Test
    fun onlyTheTagsThePhoneListsAreLanguages() {
        assertTrue(supportedTags().containsAll(listOf("en", "es", "fr", "de", "zh", "ja")))
        assertEquals("es", mlKitLanguage("es"))
        assertEquals("zh", mlKitLanguage("zh"))
        assertNull(mlKitLanguage("es-419"))
        assertNull(mlKitLanguage("xx"))
        assertNull(mlKitLanguage(""))
        assertNull(mlKitLanguage("ES"))
    }

    // English is ML Kit's pivot and is always on the phone: a pair needs the other models only.
    @Test
    fun aPairNeedsTheModelsOfItsLanguagesButEnglish() {
        assertEquals(setOf("es", "fr"), modelsFor("es", "fr"))
        assertEquals(setOf("es"), modelsFor("es", "en"))
        assertEquals(setOf("es"), modelsFor("en", "es"))
        assertEquals(emptySet<String>(), modelsFor("en", "en"))
        assertNull(modelsFor("es", "xx"))
        assertNull(modelsFor("es-419", "en"))
    }

    // Per pair, as Apple says it too: on the phone, after a download, or not here at all.
    @Test
    fun aPairIsInstalledDownloadableOrUnsupported() {
        assertEquals("installed", statusOf("es", "fr", setOf("es", "fr")))
        assertEquals("installed", statusOf("es", "en", setOf("es")))
        assertEquals("installed", statusOf("en", "es", setOf("es", "de")))
        assertEquals("downloadable", statusOf("es", "fr", setOf("es")))
        assertEquals("downloadable", statusOf("es", "en", emptySet()))
        assertEquals("unsupported", statusOf("es", "xx", setOf("es")))
        assertEquals("unsupported", statusOf("es-419", "en", setOf("es")))
        assertEquals("unsupported", statusOf("", "en", emptySet()))
        // A text already in the language asked for needs nothing.
        assertEquals("installed", statusOf("fr", "fr", emptySet()))
        assertEquals("installed", statusOf("en", "en", emptySet()))
    }

    // `prepare` downloads only what the pair lacks; a pair in one language lacks nothing.
    @Test
    fun aPairLacksOnlyTheModelsNotOnThePhone() {
        assertEquals(setOf("fr"), missingModels("es", "fr", setOf("es", "de")))
        assertEquals(setOf("es", "fr"), missingModels("es", "fr", emptySet()))
        assertEquals(emptySet<String>(), missingModels("es", "en", setOf("es")))
        assertEquals(emptySet<String>(), missingModels("fr", "fr", emptySet()))
        assertNull(missingModels("es", "xx", emptySet()))
    }

    // ML Kit queues its downloads with Android's DownloadManager, which waits for a network
    // instead of failing: offline, `prepare` says no at once rather than leaving the plugin
    // waiting for minutes.
    @Test
    fun aDownloadStartsOnlyWithANetwork() {
        assertEquals(PrepareStep.DOWNLOAD, prepareStep(setOf("fr"), online = true))
        assertEquals(PrepareStep.CANNOT, prepareStep(setOf("fr"), online = false))
        assertEquals(PrepareStep.READY, prepareStep(emptySet(), online = false))
        assertEquals(PrepareStep.CANNOT, prepareStep(null, online = true))
    }

    // `remove` deletes the downloaded models of the pair's languages other than English, which is
    // the pivot and is never deleted; the plugin frees one language with `remove(tag, "en")`. A
    // pair with nothing but English has nothing to remove (`removed: false`).
    @Test
    fun removingAPairDeletesItsModelsOnThePhoneButEnglish() {
        assertEquals(setOf("es", "fr"), removableModels("es", "fr", setOf("es", "fr", "de")))
        assertEquals(setOf("es"), removableModels("es", "fr", setOf("es")))
        assertEquals(setOf("es"), removableModels("es", "en", setOf("es", "de")))
        assertEquals(setOf("es"), removableModels("en", "es", setOf("es")))
        assertEquals(emptySet<String>(), removableModels("es", "en", emptySet()))
        assertNull(removableModels("en", "en", setOf("es")))
        assertNull(removableModels("es", "xx", setOf("es")))
    }

    // What `translateDetect` resolves with (`Detected` in platform/src/lib.rs). ML Kit says
    // "und" when it cannot tell.
    @Test
    fun aGuessGoesBackOnlyWhenTheIdentifierNamedALanguage() {
        assertEquals(mapOf<String, Any>("found" to true, "tag" to "es"), detectPayload("es"))
        // The identifier may use the old codes for Hebrew, Indonesian and Norwegian: the tag goes
        // back as the translator lists it, so the plugin finds it in `languages`.
        assertEquals(mapOf<String, Any>("found" to true, "tag" to "he"), detectPayload("iw"))
        assertEquals(mapOf<String, Any>("found" to true, "tag" to "id"), detectPayload("in"))
        // A language the translator does not have is still a guess; its pair is then unsupported.
        assertEquals(mapOf<String, Any>("found" to true, "tag" to "zh-Latn"), detectPayload("zh-Latn"))
        assertEquals("unsupported", statusOf("zh-Latn", "en", emptySet()))
        assertEquals(mapOf<String, Any>("found" to false), detectPayload("und"))
        assertEquals(mapOf<String, Any>("found" to false), detectPayload(""))
        assertEquals(mapOf<String, Any>("found" to false), detectPayload(null))
    }

    // A guess below even odds is no guess (ML Kit's own default).
    @Test
    fun aGuessNeedsAtLeastEvenOdds() {
        assertEquals(0.5f, DETECT_CONFIDENCE)
    }

    // What `translateText` resolves with (`Translated` in platform/src/lib.rs).
    @Test
    fun aTranslationGoesBackAsFoundAndNothingAsNotFound() {
        assertEquals(mapOf<String, Any>("found" to true, "text" to "hello"), textPayload("hello"))
        assertEquals(mapOf<String, Any>("found" to false), textPayload(null))
    }

    // ML Kit's translator and language identifier, at the versions the design was checked with.
    @Test
    fun mlKitTranslatesAndIdentifiesOnThePhone() {
        val gradle = File("build.gradle.kts").readText()
        assertTrue(gradle.contains("implementation(\"com.google.mlkit:translate:17.0.3\")"))
        assertTrue(gradle.contains("implementation(\"com.google.mlkit:language-id:17.0.6\")"))
    }

    // The commands Rust runs (`run_mobile_plugin` in platform/src/lib.rs) and the argument names
    // it sends: a name that differs would only show on a phone.
    @Test
    fun theTranslateCommandsAndTheirArgumentsHaveTheNamesRustSends() {
        val commands = PlatformPlugin::class.java.declaredMethods
            .filter { it.isAnnotationPresent(Command::class.java) && it.parameterTypes.toList() == listOf(Invoke::class.java) }
            .map { it.name }
        for (name in listOf("translateLanguages", "translateDetect", "translateStatus", "translatePrepare", "translateRemove", "translateText")) {
            assertTrue(name, name in commands)
        }
        fun fields(type: Class<*>) = type.declaredFields.map { it.name }.toSet()
        assertEquals(setOf("from", "to"), fields(TranslatePairArgs::class.java))
        assertEquals(setOf("text"), fields(TranslateSourceArgs::class.java))
        assertEquals(setOf("text", "from", "to"), fields(TranslateTextArgs::class.java))
    }

    // ML Kit logs a diagnostic event for every model it checks: a status asked through
    // `getDownloadedModels` queued 58 events for Google, one per language, and the Downloaded
    // languages screen about 3,400 (QA on the Lenovo, 2026-10-20). The bridge asks only about the
    // models of the pair, one by one.
    @Test
    fun theBridgeAsksMlKitOnlyAboutThePairsOwnModels() {
        val translate = File("src/main/java/Translate.kt").readText()
        assertFalse(translate.contains("getDownloadedModels"))
        assertTrue(translate.contains("isModelDownloaded"))
    }

    // ML Kit runs only when the user uses the translator: the plugin itself never touches it, and
    // Translate.kt keeps no client in a property or an object, so nothing of ML Kit is created
    // when the app starts. Each translate command makes its own client and closes it.
    @Test
    fun mlKitIsReachedOnlyFromTheTranslateCommands() {
        assertFalse(File("src/main/java/PlatformPlugin.kt").readText().contains("com.google.mlkit"))
        val translate = File("src/main/java/Translate.kt").readLines()
        val held = translate.filter { line ->
            val declaration = line.trimStart().removePrefix("private ").removePrefix("internal ")
            val topLevel = line.isNotBlank() && !line.first().isWhitespace()
            declaration.startsWith("object ") || declaration.startsWith("companion object") ||
                (topLevel && (declaration.startsWith("val ") || declaration.startsWith("var ")))
        }
        assertEquals(emptyList<String>(), held)
    }
}
