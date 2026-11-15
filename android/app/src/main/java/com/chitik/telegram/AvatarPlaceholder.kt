package com.chitik.telegram

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import java.nio.charset.StandardCharsets
import java.util.Locale

private const val AVATAR_PALETTE_SIZE = 5

/** Stable initials and palette choice derived from the supplied display name. */
data class AvatarPlaceholderStyle(
    val initials: String,
    val paletteIndex: Int,
)

/** Creates deterministic initials and a palette index without external avatar data. */
fun avatarPlaceholderStyle(displayName: String): AvatarPlaceholderStyle {
    val words = displayName.trim().split(Regex("\\s+")).filter(String::isNotEmpty)
    val initials = words.take(2).mapNotNull { word ->
        if (word.isEmpty()) {
            null
        } else {
            String(Character.toChars(word.codePointAt(0))).uppercase(Locale.ROOT)
        }
    }.joinToString("").ifBlank { "?" }

    var hash = 2_166_136_261L
    for (byte in displayName.toByteArray(StandardCharsets.UTF_8)) {
        hash = ((hash xor (byte.toLong() and 0xff)) * 16_777_619L) and 0xffff_ffffL
    }

    return AvatarPlaceholderStyle(
        initials = initials,
        paletteIndex = (hash % AVATAR_PALETTE_SIZE).toInt(),
    )
}

/** A reusable colored initial avatar for chats without a loaded profile photo. */
@Composable
fun AvatarPlaceholder(
    displayName: String,
    modifier: Modifier = Modifier,
    size: Dp = 48.dp,
    selected: Boolean = false,
) {
    val style = avatarPlaceholderStyle(displayName)
    val colors = MaterialTheme.colorScheme
    val palette = listOf(
        colors.primaryContainer to colors.onPrimaryContainer,
        colors.secondaryContainer to colors.onSecondaryContainer,
        colors.tertiaryContainer to colors.onTertiaryContainer,
        colors.surfaceVariant to colors.onSurfaceVariant,
        colors.errorContainer to colors.onErrorContainer,
    )
    val (backgroundColor, textColor) = palette[style.paletteIndex]

    Box(
        modifier = modifier
            .size(size)
            .clip(CircleShape)
            .background(if (selected) colors.primary else backgroundColor),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = style.initials,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
            color = if (selected) colors.onPrimary else textColor,
        )
    }
}
