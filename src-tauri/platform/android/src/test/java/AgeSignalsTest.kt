package com.flickertalk.platform

import com.google.android.play.agesignals.model.AgeSignalsStatus
import org.junit.Assert.assertEquals
import org.junit.Test

// 2026-10-07: minor or adult is what Play says (Play Age Signals), never what the user declares.
// Default bands are 0-12, 13-15, 16-17 and 18+: `ageUpper` is null only on the open-ended top one.
class AgeSignalsTest {
    private val shared = AgeSignalsStatus.SHARED

    @Test
    fun aBandBelowEighteenIsAMinor() {
        assertEquals("minor", ageClassOf(shared, 0, 12))
        assertEquals("minor", ageClassOf(shared, 13, 15))
        assertEquals("minor", ageClassOf(shared, 16, 17))
    }

    @Test
    fun theTopBandFromEighteenIsAnAdult() {
        assertEquals("adult", ageClassOf(shared, 18, null))
    }

    // A custom band that straddles 18 (say 16+) says neither: no guess.
    @Test
    fun aBandThatStraddlesEighteenSaysNothing() {
        assertEquals("unknown", ageClassOf(shared, 16, null))
        assertEquals("unknown", ageClassOf(shared, 15, 19))
    }

    // Not shared, verification required, no status (outside the regions, no Play) or no bounds.
    @Test
    fun anythingButASharedBandIsUnknown() {
        assertEquals("unknown", ageClassOf(AgeSignalsStatus.NOT_SHARED, 18, null))
        assertEquals("unknown", ageClassOf(AgeSignalsStatus.VERIFICATION_REQUIRED, 0, 12))
        assertEquals("unknown", ageClassOf(AgeSignalsStatus.UNSPECIFIED, 0, 12))
        assertEquals("unknown", ageClassOf(null, 18, null))
        assertEquals("unknown", ageClassOf(shared, null, null))
    }
}
