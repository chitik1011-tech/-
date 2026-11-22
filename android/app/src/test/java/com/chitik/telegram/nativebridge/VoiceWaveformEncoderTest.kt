package com.chitik.telegram.nativebridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VoiceWaveformEncoderTest {
    @Test
    fun packsLittleEndianFiveBitSamplesThatMatchTelegramDecoder() {
        val amplitudes = listOf(0, 25, 50, 100, 75, 50, 25, 100)
        val packed = packTelegramVoiceWaveform(amplitudes)

        assertEquals(listOf(0, 7, 15, 31, 23, 15, 7, 31), decodeTelegramVoiceWaveform(packed))
    }

    @Test
    fun longRecordingIsBoundedAndEmptyCaptureOmitsWaveform() {
        val packed = packTelegramVoiceWaveform((1..5000).toList())

        assertTrue(packed!!.size <= 30)
        assertEquals(48, decodeTelegramVoiceWaveform(packed).size)
        assertNull(packTelegramVoiceWaveform(emptyList()))
    }
}
