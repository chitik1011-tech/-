package com.chitik.telegram

import androidx.compose.material3.Badge
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

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
