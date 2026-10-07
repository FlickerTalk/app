package com.flickertalk.platform

import android.app.Activity
import com.google.android.play.agesignals.AgeSignalsAccessRequest
import com.google.android.play.agesignals.AgeSignalsManagerFactory
import com.google.android.play.agesignals.AgeSignalsRequest
import com.google.android.play.agesignals.model.AgeSignalsStatus

// Minor or adult, as Play says it (Ioan, 2026-10-07; Play Age Signals). Never a date of birth,
// and the answer never leaves the phone (§30, §43). Minors are always free; an adult, or someone
// Play cannot place, pays after the free year.

/** From this age on, Play's band is an adult's. */
const val ADULT_AGE = 18

/**
 * `minor`, `adult` or `unknown` from what Play answered: the access status and the band's
 * inclusive bounds (`ageUpper` is null on the open-ended top band). Only a shared band counts, and
 * a band that straddles 18 says nothing.
 */
fun ageClassOf(status: Int?, lower: Int?, upper: Int?): String = when {
    status != AgeSignalsStatus.SHARED -> "unknown"
    upper != null && upper < ADULT_AGE -> "minor"
    lower != null && lower >= ADULT_AGE -> "adult"
    else -> "unknown"
}

/**
 * Asks Play once (it may show its own sharing prompt over `activity`) and answers through
 * `answer` exactly once. No Play, an old Play, no network or a refusal are all `unknown`.
 */
fun askPlayForAge(activity: Activity, answer: (String) -> Unit) {
    val manager = try {
        AgeSignalsManagerFactory.create(activity.applicationContext)
    } catch (_: Exception) {
        answer("unknown")
        return
    }
    val access = AgeSignalsAccessRequest.builder().setActivity(activity).build()
    manager.requestAgeSignalsAccess(access).addOnCompleteListener { asked ->
        val status = if (asked.isSuccessful) asked.result?.ageSignalsStatus() else null
        if (status != AgeSignalsStatus.SHARED) {
            answer("unknown")
            return@addOnCompleteListener
        }
        manager.checkAgeSignals(AgeSignalsRequest.builder().build()).addOnCompleteListener { checked ->
            val band = if (checked.isSuccessful) checked.result else null
            answer(ageClassOf(status, band?.ageLower(), band?.ageUpper()))
        }
    }
}
