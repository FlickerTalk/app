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

// On-device translation (2026-10-20, Plan §53, app#177): ML Kit, for a plugin the user granted
// `translate` (the core checked before asking here). The text never leaves the phone. A
// language's model (~30 MB) is downloaded from Google once, only after the user agreed in the
// plugin; Google's SDK reports which languages are used and diagnostics about the app, never the
// text. Nothing here runs until a translate command does: each one makes its own ML Kit client
// and closes it. The bridge answers only for the tags it lists; any other tag is unsupported.

/** A guess below even odds is no guess (ML Kit's own default). */
const val DETECT_CONFIDENCE = 0.5f

/** The languages ML Kit translates between, as BCP-47 tags ("es", "zh"); English is its pivot. */
fun supportedTags(): List<String> = TranslateLanguage.getAllLanguages()

/** ML Kit's language for one of the tags the bridge lists; any other ("es-419", "xx", "ES") is none. */
fun mlKitLanguage(tag: String): String? = TranslateLanguage.fromLanguageTag(tag)?.takeIf { it == tag }

/** The models a pair needs on the phone, English aside (it is always there); `null` if a tag is not ours. */
fun modelsFor(from: String, to: String): Set<String>? {
    val source = mlKitLanguage(from) ?: return null
    val target = mlKitLanguage(to) ?: return null
    return setOf(source, target) - TranslateLanguage.ENGLISH
}

/** What a pair still lacks; nothing for a text already in the language asked for. */
fun missingModels(from: String, to: String, downloaded: Set<String>): Set<String>? {
    val needed = modelsFor(from, to) ?: return null
    return if (from == to) emptySet() else needed - downloaded
}

/** A pair's state, per pair as Apple gives it (`TranslateStatus` in platform/src/lib.rs). */
fun statusOf(from: String, to: String, downloaded: Set<String>): String {
    val missing = missingModels(from, to, downloaded) ?: return "unsupported"
    return if (missing.isEmpty()) "installed" else "downloadable"
}

/**
 * The pair's models on the phone, which `remove` deletes: its languages other than English, the
 * pivot, which is never deleted. `null` when there is nothing to remove: a tag that is not ours,
 * or a pair with nothing but English.
 */
fun removableModels(from: String, to: String, downloaded: Set<String>): Set<String>? =
    modelsFor(from, to)?.takeIf { it.isNotEmpty() }?.intersect(downloaded)

enum class PrepareStep { READY, DOWNLOAD, CANNOT }

/**
 * What `prepare` does with what the pair lacks. ML Kit queues its downloads with Android's
 * DownloadManager, which waits for a network instead of failing: offline, the answer is no at once.
 */
fun prepareStep(missing: Set<String>?, online: Boolean): PrepareStep = when {
    missing == null -> PrepareStep.CANNOT
    missing.isEmpty() -> PrepareStep.READY
    online -> PrepareStep.DOWNLOAD
    else -> PrepareStep.CANNOT
}

/**
 * What `translateDetect` resolves with (`Detected` in platform/src/lib.rs): the identifier's guess,
 * named as the translator lists it ("iw" is "he"); a language it does not have stays as guessed.
 */
fun detectPayload(tag: String?): Map<String, Any> {
    if (tag.isNullOrBlank() || tag == LanguageIdentifier.UNDETERMINED_LANGUAGE_TAG) return mapOf("found" to false)
    return mapOf("found" to true, "tag" to (TranslateLanguage.fromLanguageTag(tag) ?: tag))
}

/** What `translateText` resolves with (`Translated` in platform/src/lib.rs). */
fun textPayload(text: String?): Map<String, Any> =
    if (text == null) mapOf("found" to false) else mapOf("found" to true, "text" to text)

/** Which translation models are on the phone, by language; `null` when ML Kit cannot say. */
private fun downloadedModels(done: (Set<String>?) -> Unit) {
    RemoteModelManager.getInstance().getDownloadedModels(TranslateRemoteModel::class.java)
        .addOnSuccessListener { models -> done(models.map { it.language }.toSet()) }
        .addOnFailureListener { done(null) }
}

/** Whether the phone has a network now; if it cannot tell, ML Kit gets to try. */
private fun online(context: Context): Boolean = try {
    val connectivity = context.getSystemService(ConnectivityManager::class.java)
    val network = connectivity?.activeNetwork
    network != null && connectivity.getNetworkCapabilities(network)?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) == true
} catch (error: SecurityException) {
    true
}

/** `translateStatus`: the pair's state; `null` when ML Kit could not list its models. */
fun pairStatus(from: String, to: String, done: (String?) -> Unit) {
    if (modelsFor(from, to) == null) return done("unsupported")
    downloadedModels { downloaded -> done(downloaded?.let { statusOf(from, to, it) }) }
}

/**
 * `translatePrepare`: downloads what the pair lacks, on any network (the user agreed to the size in
 * the plugin), and says whether the pair can be used now.
 */
fun preparePair(context: Context, from: String, to: String, done: (Boolean) -> Unit) {
    if (modelsFor(from, to) == null) return done(false)
    downloadedModels { downloaded ->
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

/**
 * `translateRemove`: deletes the downloaded models of the pair's languages other than English (the
 * plugin frees one language with `remove(tag, "en")`); `true` once none of them is on the phone.
 */
fun removePair(from: String, to: String, done: (Boolean) -> Unit) {
    if (removableModels(from, to, emptySet()) == null) return done(false)
    downloadedModels { downloaded ->
        val removable = downloaded?.let { removableModels(from, to, it) } ?: return@downloadedModels done(false)
        val manager = RemoteModelManager.getInstance()
        Tasks.whenAll(removable.map { manager.deleteDownloadedModel(TranslateRemoteModel.Builder(it).build()) })
            .addOnSuccessListener { done(true) }
            .addOnFailureListener { done(false) }
    }
}

/** `translateDetect`: guesses the text's language on the phone; `null` when it cannot tell. */
fun detectLanguage(text: String, done: (String?) -> Unit) {
    val identifier = LanguageIdentification.getClient(
        LanguageIdentificationOptions.Builder().setConfidenceThreshold(DETECT_CONFIDENCE).build(),
    )
    identifier.identifyLanguage(text)
        .addOnSuccessListener { tag -> done(tag) }
        .addOnFailureListener { done(null) }
        .addOnCompleteListener { identifier.close() }
}

/**
 * `translateText`: translates on the phone, only with the pair's models already on it (a
 * translation never starts a download); `null` when it could not.
 */
fun translateOnPhone(text: String, from: String, to: String, done: (String?) -> Unit) {
    if (modelsFor(from, to) == null) return done(null)
    if (from == to) return done(text)
    downloadedModels { downloaded ->
        if (downloaded == null || statusOf(from, to, downloaded) != "installed") return@downloadedModels done(null)
        val options = TranslatorOptions.Builder().setSourceLanguage(from).setTargetLanguage(to).build()
        val translator = Translation.getClient(options)
        translator.translate(text)
            .addOnSuccessListener { translated -> done(translated) }
            .addOnFailureListener { done(null) }
            .addOnCompleteListener { translator.close() }
    }
}
