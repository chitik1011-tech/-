package com.chitik.telegram

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/**
 * Telegram-style presentation tokens. The backend owns all chat/session data;
 * these tokens only control Compose appearance. The default chat row follows
 * Telegram Android's DialogCell 70 dp height and 52 dp avatar diameter.
 */
object TelegramUiTokens {
    val avatar = 52.dp
    val contactAvatar = 50.dp
    val chatRowHeight = 70.dp
    val horizontalInset = 12.dp
    val rowGap = 12.dp
    val dividerInset = 76.dp
    val chatRowVerticalInset = 8.dp
    val chatSearchInset = 12.dp
    val chatSearchHeight = 48.dp
    val chatSearchCorner = 24.dp
    val chatSearchHorizontalInset = 16.dp
    val messageCorner = 16.dp
    val messageTailCorner = 5.dp
    val composerCorner = 24.dp
    val iconTouchTarget = 48.dp
    val animationShortMs = 160
    val animationMediumMs = 240
}

private val telegramLight = lightColorScheme(
    primary = Color(0xFF3390EC),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFE0F0FF),
    onPrimaryContainer = Color(0xFF152E43),
    secondary = Color(0xFF3390EC),
    secondaryContainer = Color(0xFFE7F2FB),
    onSecondaryContainer = Color(0xFF182F40),
    background = Color(0xFFFFFFFF),
    onBackground = Color(0xFF161C23),
    surface = Color(0xFFFFFFFF),
    onSurface = Color(0xFF161C23),
    surfaceVariant = Color(0xFFF1F3F5),
    onSurfaceVariant = Color(0xFF7D8995),
    outlineVariant = Color(0xFFE9EDF0),
)
private val telegramDark = darkColorScheme(
    primary = Color(0xFF70B5F5),
    onPrimary = Color(0xFF102A40),
    primaryContainer = Color(0xFF2B5278),
    onPrimaryContainer = Color.White,
    secondary = Color(0xFF70B5F5),
    secondaryContainer = Color(0xFF2B5278),
    onSecondaryContainer = Color(0xFFE4EDF2),
    background = Color(0xFF17212B),
    onBackground = Color(0xFFF1F3F5),
    surface = Color(0xFF17212B),
    onSurface = Color(0xFFF1F3F5),
    surfaceVariant = Color(0xFF243340),
    onSurfaceVariant = Color(0xFF93A4B1),
    outlineVariant = Color(0xFF2B3A47),
)

@Composable
fun TelegramAppTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (isSystemInDarkTheme()) telegramDark else telegramLight,
        content = content,
    )
}
