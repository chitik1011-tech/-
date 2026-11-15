package com.chitik.telegram.deeplink

import java.net.URI
import java.net.URLDecoder
import java.nio.charset.StandardCharsets

/**
 * A normalized, in-memory destination parsed from a Telegram link.
 *
 * Parsing never performs network I/O, joins an invite, or persists link data.
 */
sealed interface TelegramLinkTarget {
    data class Peer(val username: String, val startParameter: String? = null) : TelegramLinkTarget
    data class Message(
        val username: String,
        val messageId: Long,
        val threadId: Long? = null,
        val commentId: Long? = null,
        val single: Boolean = false,
    ) : TelegramLinkTarget
    data class PrivateChannelMessage(val channelId: Long, val messageId: Long) : TelegramLinkTarget
    data class Invite(val hash: String) : TelegramLinkTarget
    data class ComposeMessage(
        val recipient: String?,
        val text: String?,
        val url: String?,
    ) : TelegramLinkTarget
}

data class TelegramLinkIntent(val target: TelegramLinkTarget) {
    /** Invite links require a separate, explicit user confirmation before a join command. */
    val requiresConfirmation: Boolean
        get() = target is TelegramLinkTarget.Invite
}

object TelegramDeepLinkRouter {
    const val VIEW_ACTION = "android.intent.action.VIEW"

    /** Ignore non-VIEW Android intents before parsing their data. */
    fun parseIntent(action: String?, data: String?): TelegramLinkIntent? =
        if (action == VIEW_ACTION) parse(data) else null

    private const val MAX_URI_LENGTH = 4096
    private const val MAX_TEXT_LENGTH = 4096
    private val usernamePattern = Regex("^[A-Za-z][A-Za-z0-9_]{4,31}$")
    private val invitePattern = Regex("^[A-Za-z0-9_-]{1,256}$")

    /**
     * Parse a raw Android Intent data string into an app-level route.
     * Only t.me/telegram.me HTTP(S) links and supported tg:// routes are accepted.
     */
    fun parse(raw: String?): TelegramLinkIntent? {
        if (raw.isNullOrBlank() || raw.length > MAX_URI_LENGTH || raw != raw.trim()) return null
        val normalizedRaw = if (
            raw.startsWith("t.me/", ignoreCase = true) ||
            raw.startsWith("www.t.me/", ignoreCase = true) ||
            raw.startsWith("telegram.me/", ignoreCase = true) ||
            raw.startsWith("www.telegram.me/", ignoreCase = true)
        ) {
            "https://$raw"
        } else {
            raw
        }
        val uri = runCatching { URI(normalizedRaw) }.getOrNull() ?: return null
        if (uri.rawUserInfo != null || uri.rawFragment != null) return null

        return when (uri.scheme?.lowercase()) {
            "tg" -> parseTelegramScheme(uri)
            "https" -> parseWebLink(uri)
            "http" -> parseWebLink(uri)
            null -> parseWebLink(runCatching { URI("https://$raw") }.getOrNull() ?: return null)
            else -> null
        }
    }

    private fun parseTelegramScheme(uri: URI): TelegramLinkIntent? {
        if (uri.port != -1 || uri.rawPath.orEmpty().isNotEmpty()) return null
        val parameters = parseQuery(uri.rawQuery) ?: return null
        return when (uri.host?.lowercase()) {
            "resolve" -> {
                val username = parameters["domain"]?.takeIf(::isUsername) ?: return null
                val start = parameters["start"]?.let { it.takeIf { value -> value.length <= 128 } ?: return null }
                val post = parameters["post"]?.toPositiveLong()
                if (parameters.containsKey("post") && post == null) return null
                if (post != null) {
                    TelegramLinkIntent(TelegramLinkTarget.Message(username, post))
                } else {
                    TelegramLinkIntent(TelegramLinkTarget.Peer(username, start))
                }
            }
            "join" -> inviteIntent(parameters["invite"])
            "msg" -> {
                val recipient = parameters["to"]?.let { value -> value.takeIf(::isUsername) ?: return null }
                val text = parameters["text"]?.takeIf { it.length <= MAX_TEXT_LENGTH }
                if (parameters.containsKey("text") && text == null) return null
                val url = parameters["url"]?.let(::safeWebUrl)
                    ?: if (parameters.containsKey("url")) return null else null
                if (recipient == null && text == null && url == null) return null
                TelegramLinkIntent(TelegramLinkTarget.ComposeMessage(recipient, text, url))
            }
            else -> null
        }
    }

    private fun parseWebLink(uri: URI): TelegramLinkIntent? {
        if (uri.rawUserInfo != null || uri.port != -1) return null
        if (uri.scheme?.lowercase() !in setOf("http", "https")) return null
        val host = uri.host?.lowercase() ?: return null
        if (host !in setOf("t.me", "www.t.me", "telegram.me", "www.telegram.me")) return null
        val segments = uri.rawPath.orEmpty().split('/').filter(String::isNotEmpty).map { rawSegment ->
            val decoded = decodePathSegment(rawSegment) ?: return null
            if (decoded.contains('/') || decoded.contains('\\') || decoded == "." || decoded == "..") return null
            decoded
        }
        if (segments.isEmpty()) return null
        val parameters = parseQuery(uri.rawQuery) ?: return null

        val path = if (segments.firstOrNull() == "s") segments.drop(1) else segments
        if (path.isEmpty()) return null

        if (path.size == 1 && path[0].startsWith("+")) {
            return inviteIntent(path[0].removePrefix("+"))
        }
        if (path.size == 2 && path[0] == "joinchat") {
            return inviteIntent(path[1])
        }

        if (path.size == 3 && path[0] == "c") {
            val channelId = path[1].toPositiveLong() ?: return null
            val messageId = path[2].toPositiveLong() ?: return null
            return TelegramLinkIntent(TelegramLinkTarget.PrivateChannelMessage(channelId, messageId))
        }

        if (path.size == 2) {
            val username = path[0].takeIf(::isUsername) ?: return null
            val messageId = path[1].toPositiveLong() ?: return null
            val thread = parameters["thread"]?.let { it.toPositiveLong() ?: return null }
            val comment = parameters["comment"]?.let { it.toPositiveLong() ?: return null }
            val single = parameters["single"]?.let { value ->
                if (value.isEmpty()) true else value.toBooleanStrictOrNull() ?: return null
            } ?: false
            return TelegramLinkIntent(TelegramLinkTarget.Message(username, messageId, thread, comment, single))
        }

        if (path.size != 1) return null
        val username = path[0].takeIf(::isUsername) ?: return null
        val start = parameters["start"]?.let { it.takeIf { value -> value.length <= 128 } ?: return null }
        return TelegramLinkIntent(TelegramLinkTarget.Peer(username, start))
    }

    private fun inviteIntent(hash: String?): TelegramLinkIntent? {
        val validated = hash?.takeIf { invitePattern.matches(it) } ?: return null
        return TelegramLinkIntent(TelegramLinkTarget.Invite(validated))
    }

    private fun parseQuery(rawQuery: String?): Map<String, String>? {
        if (rawQuery.isNullOrEmpty()) return emptyMap()
        val result = linkedMapOf<String, String>()
        for (part in rawQuery.split('&')) {
            if (part.isEmpty()) return null
            val pair = part.split('=', limit = 2)
            val key = decodeQueryComponent(pair[0])?.takeIf(String::isNotEmpty) ?: return null
            val value = decodeQueryComponent(pair.getOrElse(1) { "" }) ?: return null
            if (result.putIfAbsent(key, value) != null) return null
        }
        return result
    }

    private fun safeWebUrl(raw: String): String? {
        val uri = runCatching { URI(raw) }.getOrNull() ?: return null
        if (uri.scheme?.lowercase() !in setOf("http", "https")) return null
        if (uri.host.isNullOrBlank() || uri.rawUserInfo != null) return null
        return uri.toASCIIString().takeIf { it.length <= MAX_URI_LENGTH }
    }

    private fun isUsername(value: String): Boolean = usernamePattern.matches(value)

    private fun String.toPositiveLong(): Long? = toLongOrNull()?.takeIf { it > 0 }

    private fun decodePathSegment(value: String): String? =
        runCatching {
            URLDecoder.decode(value.replace("+", "%2B"), StandardCharsets.UTF_8.name())
        }.getOrNull()

    private fun decodeQueryComponent(value: String): String? =
        runCatching { URLDecoder.decode(value, StandardCharsets.UTF_8.name()) }.getOrNull()
}