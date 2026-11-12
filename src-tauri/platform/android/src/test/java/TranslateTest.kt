package com.flickertalk.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import app.tauri.annotation.Command
import app.tauri.plugin.Invoke
import java.io.File

class TranslateTest {
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

    @Test
    fun aPairNeedsTheModelsOfItsLanguagesButEnglish() {
        assertEquals(setOf("es", "fr"), modelsFor("es", "fr"))
        assertEquals(setOf("es"), modelsFor("es", "en"))
        assertEquals(setOf("es"), modelsFor("en", "es"))
        assertEquals(emptySet<String>(), modelsFor("en", "en"))
        assertNull(modelsFor("es", "xx"))
        assertNull(modelsFor("es-419", "en"))
    }

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
        assertEquals("installed", statusOf("fr", "fr", emptySet()))
        assertEquals("installed", statusOf("en", "en", emptySet()))
    }

    @Test
    fun aPairLacksOnlyTheModelsNotOnThePhone() {
        assertEquals(setOf("fr"), missingModels("es", "fr", setOf("es", "de")))
        assertEquals(setOf("es", "fr"), missingModels("es", "fr", emptySet()))
        assertEquals(emptySet<String>(), missingModels("es", "en", setOf("es")))
        assertEquals(emptySet<String>(), missingModels("fr", "fr", emptySet()))
        assertNull(missingModels("es", "xx", emptySet()))
    }

    @Test
    fun aDownloadStartsOnlyWithANetwork() {
        assertEquals(PrepareStep.DOWNLOAD, prepareStep(setOf("fr"), online = true))
        assertEquals(PrepareStep.CANNOT, prepareStep(setOf("fr"), online = false))
        assertEquals(PrepareStep.READY, prepareStep(emptySet(), online = false))
        assertEquals(PrepareStep.CANNOT, prepareStep(null, online = true))
    }

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

    @Test
    fun aGuessGoesBackOnlyWhenTheIdentifierNamedALanguage() {
        assertEquals(mapOf<String, Any>("found" to true, "tag" to "es"), detectPayload("es"))
        assertEquals(mapOf<String, Any>("found" to true, "tag" to "he"), detectPayload("iw"))
        assertEquals(mapOf<String, Any>("found" to true, "tag" to "id"), detectPayload("in"))
        assertEquals(mapOf<String, Any>("found" to true, "tag" to "zh-Latn"), detectPayload("zh-Latn"))
        assertEquals("unsupported", statusOf("zh-Latn", "en", emptySet()))
        assertEquals(mapOf<String, Any>("found" to false), detectPayload("und"))
        assertEquals(mapOf<String, Any>("found" to false), detectPayload(""))
        assertEquals(mapOf<String, Any>("found" to false), detectPayload(null))
    }

    @Test
    fun aGuessNeedsAtLeastEvenOdds() {
        assertEquals(0.5f, DETECT_CONFIDENCE)
    }

    @Test
    fun aTranslationGoesBackAsFoundAndNothingAsNotFound() {
        assertEquals(mapOf<String, Any>("found" to true, "text" to "hello"), textPayload("hello"))
        assertEquals(mapOf<String, Any>("found" to false), textPayload(null))
    }

    @Test
    fun mlKitTranslatesAndIdentifiesOnThePhone() {
        val gradle = File("build.gradle.kts").readText()
        assertTrue(gradle.contains("implementation(\"com.google.mlkit:translate:17.0.3\")"))
        assertTrue(gradle.contains("implementation(\"com.google.mlkit:language-id:17.0.6\")"))
    }

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

    @Test
    fun theBridgeAsksMlKitOnlyAboutThePairsOwnModels() {
        val translate = File("src/main/java/Translate.kt").readText()
        assertFalse(translate.contains("getDownloadedModels"))
        assertTrue(translate.contains("isModelDownloaded"))
    }

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
