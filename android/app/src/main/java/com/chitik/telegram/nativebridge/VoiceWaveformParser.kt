package com.chitik.telegram.nativebridge

/** Unpacks Telegram's little-endian 5-bit voice amplitude samples from its byte array. */
internal fun decodeTelegramVoiceWaveform(
    packed: List<Int>?,
    maxSamples: Int = 48,
): List<Int> {
    if (packed.isNullOrEmpty() || packed.size > 4096 || maxSamples <= 0) return emptyList()
    if (packed.any { it !in 0..255 }) return emptyList()
    val sampleCount = minOf((packed.size * 8) / 5, maxSamples, 48)
    return List(sampleCount) { index ->
        val bitOffset = index * 5
        val byteIndex = bitOffset / 8
        val shift = bitOffset % 8
        val low = packed[byteIndex] ushr shift
        val high = if (shift == 0) 0 else packed.getOrElse(byteIndex + 1) { 0 } shl (8 - shift)
        (low or high) and 0x1f
    }
}
