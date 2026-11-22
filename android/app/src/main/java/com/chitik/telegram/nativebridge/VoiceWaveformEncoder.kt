package com.chitik.telegram.nativebridge

/** Packs sampled microphone peaks into Telegram's little-endian 5-bit waveform bytes. */
internal fun packTelegramVoiceWaveform(samples: List<Int>): List<Int>? {
    if (samples.isEmpty()) return null
    val bars = minOf(samples.size, MAX_TELEGRAM_WAVEFORM_SAMPLES)
    val peak = samples.maxOf { it.coerceAtLeast(0) }
    val normalized = List(bars) { bar ->
        val start = bar * samples.size / bars
        val end = ((bar + 1) * samples.size / bars).coerceAtLeast(start + 1)
        val bucketPeak = samples.subList(start, end.coerceAtMost(samples.size))
            .maxOf { it.coerceAtLeast(0) }
        if (peak == 0) 0 else ((bucketPeak.toDouble() / peak) * 31.0).toInt().coerceIn(0, 31)
    }
    val packed = ByteArray((bars * TELEGRAM_WAVEFORM_BITS + 7) / 8)
    normalized.forEachIndexed { index, amplitude ->
        val bitOffset = index * TELEGRAM_WAVEFORM_BITS
        val byteIndex = bitOffset / 8
        val shift = bitOffset % 8
        packed[byteIndex] = (packed[byteIndex].toInt() or (amplitude shl shift)).toByte()
        if (shift > 3) {
            packed[byteIndex + 1] = (
                packed[byteIndex + 1].toInt() or (amplitude ushr (8 - shift))
            ).toByte()
        }
    }
    return packed.map { it.toInt() and 0xff }
}

private const val TELEGRAM_WAVEFORM_BITS = 5
private const val MAX_TELEGRAM_WAVEFORM_SAMPLES = 48
