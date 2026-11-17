package com.chitik.telegram

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Badge
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AlternateEmail
import androidx.compose.material.icons.outlined.EmojiEmotions
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/** Returns a compact unread label, or null when there is nothing unread. */
fun formatUnreadCount(count: Long): String? = when {
    count <= 0L -> null
    count > 999L -> "999+"
    else -> count.toString()
}

/** Displays an unread count using the Material theme's badge colors. */
@Composable
fun UnreadCounterBadge(
    count: Long,
    modifier: Modifier = Modifier,
) {
    val label = formatUnreadCount(count) ?: return

    Badge(
        modifier = modifier,
    ) {
        Text(label)
    }
}

/** Telegram-style mention badge; callers pass a bounded label from [formatUnreadCount]. */
@Composable
fun UnreadMentionBadge(
    label: String,
    modifier: Modifier = Modifier,
) {
    Badge(modifier = modifier) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(
                imageVector = Icons.Outlined.AlternateEmail,
                contentDescription = null,
                modifier = Modifier.size(12.dp),
            )
            Spacer(Modifier.width(2.dp))
            Text(label, maxLines = 1)
        }
    }
}

/** Telegram-style reaction badge; callers pass a bounded label from [formatUnreadCount]. */
@Composable
fun UnreadReactionBadge(
    label: String,
    modifier: Modifier = Modifier,
) {
    Badge(modifier = modifier) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(
                imageVector = Icons.Outlined.EmojiEmotions,
                contentDescription = null,
                modifier = Modifier.size(12.dp),
            )
            Spacer(Modifier.width(2.dp))
            Text(label, maxLines = 1)
        }
    }
}
