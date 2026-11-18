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
data class TelegramNotificationRoute(val chatId: Long, val accountUserId: Long) {
    init {
        require(chatId != 0L) { "chatId must be non-zero" }
        require(accountUserId > 0L) { "accountUserId must be positive" }
    }

    companion object {
        const val ACTION_OPEN_CHAT = "com.chitik.telegram.action.OPEN_CHAT"
        const val EXTRA_CHAT_ID = "com.chitik.telegram.extra.CHAT_ID"
        const val EXTRA_ACCOUNT_USER_ID = "com.chitik.telegram.extra.ACCOUNT_USER_ID"
        private const val URI_SCHEME = "tg"
        private const val URI_AUTHORITY = "chat"
        private const val QUERY_ACCOUNT_ID = "account_id"

        /**
         * Parses and validates an explicit notification route. The action, URI, and extra must
         * agree so malformed or stale intents cannot open an unintended chat.
         */
        fun parse(intent: Intent?): TelegramNotificationRoute? {
            if (intent?.action != ACTION_OPEN_CHAT) return null
            val uri = intent.data ?: return null
            val route = parse(uri) ?: return null
            if (!intent.hasExtra(EXTRA_CHAT_ID)) return null
            if (intent.getLongExtra(EXTRA_CHAT_ID, 0L) != route.chatId) return null
            if (!intent.hasExtra(EXTRA_ACCOUNT_USER_ID)) return null
            if (intent.getLongExtra(EXTRA_ACCOUNT_USER_ID, 0L) != route.accountUserId) return null
            return route
        }

        /** Parses the account-scoped tg://chat/<id>?account_id=<Telegram user ID> route. */
        fun parse(uri: Uri?): TelegramNotificationRoute? {
            if (uri?.scheme != URI_SCHEME || uri.authority != URI_AUTHORITY) return null
            if (uri.fragment != null) return null
            if (uri.pathSegments.size != 1) return null
            val chatId = uri.pathSegments.single().toLongOrNull() ?: return null
            if (chatId == 0L || uri.queryParameterNames != setOf(QUERY_ACCOUNT_ID)) return null
            val accountIds = uri.getQueryParameters(QUERY_ACCOUNT_ID)
            if (accountIds.size != 1) return null
            val accountUserId = accountIds.single().toLongOrNull()?.takeIf { it > 0L } ?: return null
            return TelegramNotificationRoute(chatId, accountUserId)
        }

        /** Creates an explicit MainActivity intent scoped to the posting Telegram account. */
        fun createIntent(context: Context, chatId: Long, accountUserId: Long): Intent {
            require(chatId != 0L) { "chatId must be non-zero" }
            require(accountUserId > 0L) { "accountUserId must be positive" }
            val uri = Uri.Builder()
                .scheme(URI_SCHEME)
                .authority(URI_AUTHORITY)
                .appendPath(chatId.toString())
                .appendQueryParameter(QUERY_ACCOUNT_ID, accountUserId.toString())
                .build()
            return Intent(Intent.ACTION_VIEW, uri)
                .setClassName(context, "${context.packageName}.MainActivity")
                .setAction(ACTION_OPEN_CHAT)
                .putExtra(EXTRA_CHAT_ID, chatId)
                .putExtra(EXTRA_ACCOUNT_USER_ID, accountUserId)
                .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }

        /**
         * Returns an immutable PendingIntent unique to a chat and Telegram account. The URI
         * participates in identity, so routes for different chats or accounts do not collide.
         */
        fun createPendingIntent(context: Context, chatId: Long, accountUserId: Long): PendingIntent {
            val intent = createIntent(context, chatId, accountUserId)
            return PendingIntent.getActivity(
                context,
                chatId.hashCode(),
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }
    }
}
