package com.chitik.telegram.notifications

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri

/**
 * Identifies a chat opened from a Telegram message notification.
 *
 * The route is intentionally independent from navigation so the Android shell can consume
 * it from both [android.app.Activity.onCreate] and [android.app.Activity.onNewIntent].
 */
data class TelegramNotificationRoute(val chatId: Long) {
    init {
        require(chatId != 0L) { "chatId must be non-zero" }
    }

    companion object {
        const val ACTION_OPEN_CHAT = "com.chitik.telegram.action.OPEN_CHAT"
        const val EXTRA_CHAT_ID = "com.chitik.telegram.extra.CHAT_ID"
        private const val URI_SCHEME = "tg"
        private const val URI_AUTHORITY = "chat"

        /**
         * Parses and validates an explicit notification route. The action, URI, and extra must
         * agree so malformed or stale intents cannot open an unintended chat.
         */
        fun parse(intent: Intent?): TelegramNotificationRoute? {
            if (intent?.action != ACTION_OPEN_CHAT) return null
            val uri = intent.data ?: return null
            val chatId = parse(uri) ?: return null
            if (!intent.hasExtra(EXTRA_CHAT_ID)) return null
            if (intent.getLongExtra(EXTRA_CHAT_ID, 0L) != chatId) return null
            return TelegramNotificationRoute(chatId)
        }

        /** Parses the stable tg://chat/<id> route form. */
        fun parse(uri: Uri?): Long? {
            if (uri?.scheme != URI_SCHEME || uri.authority != URI_AUTHORITY) return null
            if (uri.query != null || uri.fragment != null) return null
            if (uri.pathSegments.size != 1) return null
            val chatId = uri.pathSegments.single().toLongOrNull() ?: return null
            return chatId.takeIf { it != 0L }
        }

        /** Creates an explicit MainActivity intent for one chat. */
        fun createIntent(context: Context, chatId: Long): Intent {
            require(chatId != 0L) { "chatId must be non-zero" }
            val uri = Uri.Builder()
                .scheme(URI_SCHEME)
                .authority(URI_AUTHORITY)
                .appendPath(chatId.toString())
                .build()
            return Intent(Intent.ACTION_VIEW, uri)
                .setClassName(context, "${context.packageName}.MainActivity")
                .setAction(ACTION_OPEN_CHAT)
                .putExtra(EXTRA_CHAT_ID, chatId)
                .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }

        /**
         * Returns an immutable PendingIntent unique to a chat. The URI participates in identity,
         * so notifications for different chats do not overwrite one another.
         */
        fun createPendingIntent(context: Context, chatId: Long): PendingIntent {
            val intent = createIntent(context, chatId)
            return PendingIntent.getActivity(
                context,
                chatId.hashCode(),
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }
    }
}
