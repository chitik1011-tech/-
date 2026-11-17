package com.chitik.telegram

import java.util.Locale

internal data class PeerProfilePermission(
    val label: String,
    val value: String,
)

internal data class PeerProfilePresentation(
    val title: String,
    val kindLabel: String,
    val presence: String?,
    val username: String?,
    val bio: String?,
    val memberCount: Int?,
    val badges: List<String>,
    val permissions: List<PeerProfilePermission>,
    val canMessage: Boolean,
)

/** Projects only values actually supplied by Telegram; missing fields remain hidden. */
internal fun PeerProfileUi.toPeerProfilePresentation(): PeerProfilePresentation {
    val cleanDisplayName = displayName.cleanProfileValue()
    val composedName = listOfNotNull(firstName.cleanProfileValue(), lastName.cleanProfileValue())
        .joinToString(" ")
        .takeIf(String::isNotBlank)

    return PeerProfilePresentation(
        title = cleanDisplayName ?: composedName ?: username.cleanProfileValue()?.removePrefix("@") ?: "Profile",
        kindLabel = kind.toProfileKindLabel(),
        presence = presence.cleanProfileValue()?.let(::normalizePresence),
        username = username.cleanProfileValue()?.removePrefix("@"),
        bio = bio.cleanProfileValue(),
        memberCount = memberCount?.takeIf { it >= 0 },
        badges = buildList {
            if (isVerified == true) add("Verified")
            if (isPremium == true) add("Premium")
            if (isBot == true || kind.equals("bot", ignoreCase = true)) add("Bot")
        },
        permissions = buildList {
            canSendMessages?.let { add(PeerProfilePermission("Can send messages", it.toPermissionLabel())) }
            canInviteUsers?.let { add(PeerProfilePermission("Can add members", it.toPermissionLabel())) }
            canChangeInfo?.let { add(PeerProfilePermission("Can edit info", it.toPermissionLabel())) }
            canPostMessages?.let { add(PeerProfilePermission("Can post messages", it.toPermissionLabel())) }
        },
        canMessage = when {
            kind.equals("channel", ignoreCase = true) -> canSendMessages == true
            else -> canSendMessages != false
        },
    )
}

private fun String?.cleanProfileValue(): String? = this?.trim()?.takeIf(String::isNotEmpty)

private fun String.toProfileKindLabel(): String = when (lowercase(Locale.ROOT)) {
    "user" -> "Private chat"
    "basic_group" -> "Group"
    "supergroup" -> "Supergroup"
    "channel" -> "Channel"
    "bot" -> "Bot"
    else -> "Telegram profile"
}

private fun normalizePresence(value: String): String = when {
    value.equals("online", ignoreCase = true) -> "Online"
    value.equals("offline", ignoreCase = true) -> "Offline"
    else -> value
}

private fun Boolean.toPermissionLabel(): String = if (this) "Allowed" else "Unavailable"
