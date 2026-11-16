package com.flickertalk.platform

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.provider.ContactsContract.CommonDataKinds.Phone
import android.telephony.TelephonyManager
import androidx.activity.result.ActivityResult
import app.tauri.annotation.InvokeArg
import app.tauri.plugin.JSObject
import java.util.Locale

val WHATSAPP_PACKAGES = listOf("com.whatsapp", "com.whatsapp.w4b")

val NOBODY_PICKED: Map<String, Any> = mapOf("picked" to false)

fun phoneCountry(sim: String?, network: String?, region: String?): String =
    listOf(sim, network, region).firstOrNull { !it.isNullOrBlank() }?.trim()?.uppercase(Locale.ROOT) ?: ""

fun pickedContactPayload(name: String?, number: String?, country: String): Map<String, Any> =
    mapOf("picked" to true, "name" to (name ?: ""), "number" to (number ?: ""), "country" to country)

@InvokeArg
class WhatsAppArgs {
    lateinit var url: String
}

// the system picker shares only the chosen number, so no contacts permission is needed; nothing here is kept or logged
class Invite(private val current: () -> Activity) {
    private val activity: Activity get() = current()

    fun pickIntent(): Intent = Intent(Intent.ACTION_PICK, Phone.CONTENT_URI)

    fun picked(result: ActivityResult): JSObject {
        val uri = result.data?.data
        if (result.resultCode != Activity.RESULT_OK || uri == null) return jsObject(NOBODY_PICKED)
        var name: String? = null
        var number: String? = null
        activity.contentResolver.query(uri, arrayOf(Phone.DISPLAY_NAME, Phone.NUMBER), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) {
                name = cursor.getString(0)
                number = cursor.getString(1)
            }
        }
        return jsObject(pickedContactPayload(name, number, country()))
    }

    // an explicit package keeps the chat out of the browser, with no <queries> entry needed
    fun openWhatsApp(url: String): Boolean = WHATSAPP_PACKAGES.any { app ->
        try {
            activity.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).setPackage(app))
            true
        } catch (_: ActivityNotFoundException) {
            false
        }
    }

    private fun country(): String {
        val telephony = activity.getSystemService(TelephonyManager::class.java)
        return phoneCountry(telephony?.simCountryIso, telephony?.networkCountryIso, Locale.getDefault().country)
    }

    private fun jsObject(payload: Map<String, Any>) = JSObject().apply { payload.forEach { (key, value) -> put(key, value) } }
}
