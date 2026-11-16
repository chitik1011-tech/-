package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class VoiceWaveformTest {
    @Test fun decodesTelegramPackedFiveBitAmplitude() {
        assertEquals(listOf(31, 31, 0), decodeVoiceWaveform(listOf(255, 3)))
    }

    @Test fun missingAndMalformedWaveformsHaveNoFabricatedSamples() {
        assertTrue(decodeVoiceWaveform(null).isEmpty())
        assertTrue(decodeVoiceWaveform(emptyList()).isEmpty())
        assertTrue(decodeVoiceWaveform(listOf(-1)).isEmpty())
        assertTrue(decodeVoiceWaveform(listOf(256)).isEmpty())
        assertTrue(decodeVoiceWaveform(List(4097) { 0 }).isEmpty())
    }

    @Test fun waveformRenderingHasBoundedSampleCount() {
        assertEquals(7, decodeVoiceWaveform(List(10) { 0xff }, maxSamples = 7).size)
        assertTrue(decodeVoiceWaveform(List(10) { 0xff }, maxSamples = 0).isEmpty())
    }

    @Test fun compactVoiceDurationUsesExistingFormatter() {
        assertEquals("1:05", MediaDurationFormatter.format(65000))
        assertEquals("", MediaDurationFormatter.format(null))
    }
}
