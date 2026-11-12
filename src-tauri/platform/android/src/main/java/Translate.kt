package com.flickertalk.platform

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import com.google.android.gms.tasks.Tasks
import com.google.mlkit.common.model.DownloadConditions
import com.google.mlkit.common.model.RemoteModelManager
import com.google.mlkit.nl.languageid.LanguageIdentification
import com.google.mlkit.nl.languageid.LanguageIdentificationOptions
import com.google.mlkit.nl.languageid.LanguageIdentifier
import com.google.mlkit.nl.translate.TranslateLanguage
import com.google.mlkit.nl.translate.TranslateRemoteModel
import com.google.mlkit.nl.translate.Translation
import com.google.mlkit.nl.translate.TranslatorOptions

// The text never leaves the phone. Models come from Google only after the user agreed in the plugin.

const val DETECT_CONFIDENCE = 0.5f

fun supportedTags(): List<String> = TranslateLanguage.getAllLanguages()

fun mlKitLanguage(tag: String): String? = TranslateLanguage.fromLanguageTag(tag)?.takeIf { it == tag }

// English is always on the phone
fun modelsFor(from: String, to: String): Set<String>? {
    val source = mlKitLanguage(from) ?: return null
    val target = mlKitLanguage(to) ?: return null
    return setOf(source, target) - TranslateLanguage.ENGLISH
}

fun missingModels(from: String, to: String, downloaded: Set<String>): Set<String>? {
    val needed = modelsFor(from, to) ?: return null
    return if (from == to) emptySet() else needed - downloaded
}

fun statusOf(from: String, to: String, downloaded: Set<String>): String {
    val missing = missingModels(from, to, downloaded) ?: return "unsupported"
    return if (missing.isEmpty()) "installed" else "downloadable"
}

// never English, every pair needs it
fun removableModels(from: String, to: String, downloaded: Set<String>): Set<String>? =
    modelsFor(from, to)?.takeIf { it.isNotEmpty() }?.intersect(downloaded)

enum class PrepareStep { READY, DOWNLOAD, CANNOT }

// ML Kit's downloads wait for a network instead of failing, so say no right away when offline
fun prepareStep(missing: Set<String>?, online: Boolean): PrepareStep = when {
    missing == null -> PrepareStep.CANNOT
    missing.isEmpty() -> PrepareStep.READY
    online -> PrepareStep.DOWNLOAD
    else -> PrepareStep.CANNOT
}

// the identifier says "iw" where the translator lists "he"
fun detectPayload(tag: String?): Map<String, Any> {
    if (tag.isNullOrBlank() || tag == LanguageIdentifier.UNDETERMINED_LANGUAGE_TAG) return mapOf("found" to false)
    return mapOf("found" to true, "tag" to (TranslateLanguage.fromLanguageTag(tag) ?: tag))
}

fun textPayload(text: String?): Map<String, Any> =
    if (text == null) mapOf("found" to false) else mapOf("found" to true, "text" to text)

// one model at a time, ML Kit logs a diagnostic event for every model it checks and the list has 58
private fun downloadedAmong(languages: Set<String>, done: (Set<String>?) -> Unit) {
    if (languages.isEmpty()) return done(emptySet())
    val manager = RemoteModelManager.getInstance()
    val asked = languages.toList()
    Tasks.whenAllSuccess<Boolean>(asked.map { manager.isModelDownloaded(TranslateRemoteModel.Builder(it).build()) })
        .addOnSuccessListener { present -> done(asked.filterIndexed { index, _ -> present[index] == true }.toSet()) }
        .addOnFailureListener { done(null) }
}

private fun online(context: Context): Boolean = try {
    val connectivity = context.getSystemService(ConnectivityManager::class.java)
    val network = connectivity?.activeNetwork
    network != null && connectivity.getNetworkCapabilities(network)?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) == true
} catch (error: SecurityException) {
    true
}

fun pairStatus(from: String, to: String, done: (String?) -> Unit) {
    val models = modelsFor(from, to) ?: return done("unsupported")
    downloadedAmong(models) { downloaded -> done(downloaded?.let { statusOf(from, to, it) }) }
}

fun preparePair(context: Context, from: String, to: String, done: (Boolean) -> Unit) {
    val models = modelsFor(from, to) ?: return done(false)
    downloadedAmong(models) { downloaded ->
        val missing = downloaded?.let { missingModels(from, to, it) }
        when (prepareStep(missing, online(context))) {
            PrepareStep.READY -> done(true)
            PrepareStep.CANNOT -> done(false)
            PrepareStep.DOWNLOAD -> {
                val manager = RemoteModelManager.getInstance()
                val conditions = DownloadConditions.Builder().build()
                Tasks.whenAll(missing.orEmpty().map { manager.download(TranslateRemoteModel.Builder(it).build(), conditions) })
                    .addOnSuccessListener { done(true) }
                    .addOnFailureListener { done(false) }
            }
        }
    }
}

fun removePair(from: String, to: String, done: (Boolean) -> Unit) {
    val models = modelsFor(from, to)?.takeIf { it.isNotEmpty() } ?: return done(false)
    downloadedAmong(models) { downloaded ->
        val removable = downloaded?.let { removableModels(from, to, it) } ?: return@downloadedAmong done(false)
        val manager = RemoteModelManager.getInstance()
        Tasks.whenAll(removable.map { manager.deleteDownloadedModel(TranslateRemoteModel.Builder(it).build()) })
            .addOnSuccessListener { done(true) }
            .addOnFailureListener { done(false) }
    }
}

fun detectLanguage(text: String, done: (String?) -> Unit) {
    val identifier = LanguageIdentification.getClient(
        LanguageIdentificationOptions.Builder().setConfidenceThreshold(DETECT_CONFIDENCE).build(),
    )
    identifier.identifyLanguage(text)
        .addOnSuccessListener { tag -> done(tag) }
        .addOnFailureListener { done(null) }
        .addOnCompleteListener { identifier.close() }
}

// never starts a download, prepare does that once the user agreed
fun translateOnPhone(text: String, from: String, to: String, done: (String?) -> Unit) {
    val models = modelsFor(from, to) ?: return done(null)
    if (from == to) return done(text)
    downloadedAmong(models) { downloaded ->
        if (downloaded == null || statusOf(from, to, downloaded) != "installed") return@downloadedAmong done(null)
        val options = TranslatorOptions.Builder().setSourceLanguage(from).setTargetLanguage(to).build()
        val translator = Translation.getClient(options)
        translator.translate(text)
            .addOnSuccessListener { translated -> done(translated) }
            .addOnFailureListener { done(null) }
            .addOnCompleteListener { translator.close() }
    }
}
