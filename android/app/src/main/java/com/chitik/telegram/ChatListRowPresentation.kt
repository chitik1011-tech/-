package com.chitik.telegram

import java.time.Instant
import java.time.ZoneId
import java.util.Locale

/** Presentation derived only from fields already supplied by Core for a chat row. */
internal data class ChatListRowPresentation(
    val title: String,
    val timestamp: String?,
    val preview: String?,
    val outgoingPreview: Boolean,
    val unreadLabel: String?,
    val unreadMentionsLabel: String?,
    val unreadReactionsLabel: String?,
    val markedUnreadLabel: String?,
    val accessibilityLabel: String,
)

internal fun ChatListEntry.toChatListRowPresentation(
    now: Instant = Instant.now(),
    zoneId: ZoneId = ZoneId.systemDefault(),
    locale: Locale = Locale.getDefault(),
): ChatListRowPresentation {
    val message = lastMessage
    val timestamp = message?.let {
        MessageTimestampFormatter.format(it.timestampUnix, now, zoneId, locale)
    }?.takeIf(String::isNotBlank)
    val text = message?.text?.trim()?.takeIf(String::isNotEmpty)
    val preview = when {
        message == null -> "No messages yet"
        text == null -> null
        message.outgoing -> "You: $text"
        else -> text
    }
    val outgoingPreview = message?.outgoing == true && text != null
    val unreadLabel = formatUnreadCount(unreadCount)
    val unreadMentionsLabel = unreadMentionsCount?.let(::formatUnreadCount)
    val unreadReactionsLabel = unreadReactionsCount?.let(::formatUnreadCount)
    val markedUnreadLabel = if (isUnread == true) "Marked unread" else null
    val accessibilityLabel = buildList {
        add(title)
        timestamp?.let { add(it) }
        if (outgoingPreview) add("Sent")
        preview?.let { add(it) }
        unreadLabel?.let { add("$it unread") }
        unreadMentionsLabel?.let { add("$it unread mentions") }
        unreadReactionsLabel?.let { add("$it unread reactions") }
        markedUnreadLabel?.let(::add)
    }.joinToString(", ")

    return ChatListRowPresentation(
        title = title,
        timestamp = timestamp,
        preview = preview,
        outgoingPreview = outgoingPreview,
        unreadLabel = unreadLabel,
        unreadMentionsLabel = unreadMentionsLabel,
        unreadReactionsLabel = unreadReactionsLabel,
        markedUnreadLabel = markedUnreadLabel,
        accessibilityLabel = accessibilityLabel,
    )
}
