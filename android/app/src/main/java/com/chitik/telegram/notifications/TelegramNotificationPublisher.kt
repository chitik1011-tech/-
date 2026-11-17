package com.chitik.telegram.notifications

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.ContextCompat
import java.util.concurrent.atomic.AtomicReference

internal const val TELEGRAM_MESSAGE_NOTIFICATION_TAG_PREFIX = "telegram_chat_"

/**
 * Publishes message notifications from caller-supplied Rust event data.
 *
 * This class performs no Telegram I/O and stores no message or credential data. The Android
 * shell should pass chat/message fields mapped from Rust events and update [setVisibleChat] as
 * navigation changes.
 */
class TelegramNotificationPublisher(context: Context) {
    private val appContext = context.applicationContext
    private val notificationManager =
        appContext.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
    private val visibleChatId = AtomicReference<Long?>(null)
    private val notificationLock = Any()

    init {
        ensureChannels()
    }

    /**
     * Chat and message fields mapped by the caller from Rust-owned state/events.
     * A blank body is allowed for media-only messages and renders as a generic message notice.
     */
    data class IncomingMessage(
        val chatId: Long,
        val chatTitle: String,
        val body: String,
        val isGroup: Boolean,
        val senderName: String? = null,
        val timestampMillis: Long = System.currentTimeMillis(),
    )

    /** Marks the currently visible chat; its incoming events are not shown as notifications. */
    fun setVisibleChat(chatId: Long?) {
        visibleChatId.set(chatId)
        if (chatId != null) {
            synchronized(notificationLock) {
                notificationManager?.let { manager ->
                    cancelSafely(manager, notificationTag(chatId))
                    refreshGroupSummary(manager)
                }
            }
        }
    }

    /** Dismisses one chat's notification and keeps the group summary in sync. */
    fun cancelChat(chatId: Long) {
        if (chatId == 0L) return
        synchronized(notificationLock) {
            notificationManager?.let { manager ->
                cancelSafely(manager, notificationTag(chatId))
                refreshGroupSummary(manager)
            }
        }
    }

    /** Cancels only this publisher's tagged chat messages, including notices surviving process recreation. */
    fun cancelPublishedNotifications() {
        val manager = notificationManager ?: return
        synchronized(notificationLock) {
            cancelSafely(manager, TelegramMessageNotificationGrouping.SUMMARY_TAG)
            activeChatNotifications(manager)?.forEach { status ->
                status.tag?.let { cancelSafely(manager, it, status.id) }
            }
        }
    }

    /**
     * Posts a notification unless permission is unavailable or the same chat is currently open.
     * Returns true only when a notification was delivered to Android's NotificationManager.
     */
    fun publish(message: IncomingMessage): Boolean {
        if (message.chatId == 0L) return false
        if (!hasNotificationPermission()) return false
        val manager = notificationManager ?: return false
        val channelId = if (message.isGroup) GROUP_CHANNEL_ID else PRIVATE_CHANNEL_ID
        val title = if (message.isGroup) {
            message.chatTitle
        } else {
            message.senderName?.takeIf(String::isNotBlank) ?: message.chatTitle
        }.ifBlank { "Telegram" }
        val presentation = notificationPresentation(title = title, body = message.body)

        val notificationBuilder = Notification.Builder(appContext, channelId)
            .setSmallIcon(android.R.drawable.sym_action_chat)
            .setContentTitle(presentation.title)
            .setContentText(presentation.body)
            .setStyle(Notification.BigTextStyle().bigText(presentation.body))
            .setWhen(message.timestampMillis)
            .setShowWhen(true)
            .setCategory(Notification.CATEGORY_MESSAGE)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
        TelegramMessageNotificationGrouping.groupKeyForChat(message.chatId)?.let {
            notificationBuilder.setGroup(it)
            notificationBuilder.setGroupAlertBehavior(Notification.GROUP_ALERT_CHILDREN)
        }

        val notification = notificationBuilder
            .setPublicVersion(
                Notification.Builder(appContext, channelId)
                    .setSmallIcon(android.R.drawable.sym_action_chat)
                    .setContentTitle("Telegram")
                    .setContentText("New message")
                    .setVisibility(Notification.VISIBILITY_PUBLIC)
                    .build(),
            )
            .setContentIntent(
                TelegramNotificationRoute.createPendingIntent(appContext, message.chatId),
            )
            .setAutoCancel(true)
            .build()

        return synchronized(notificationLock) {
            if (visibleChatId.get() == message.chatId) return@synchronized false
            try {
                manager.notify(notificationTag(message.chatId), NOTIFICATION_ID, notification)
            } catch (_: SecurityException) {
                // Permission can be revoked between the check and notify.
                return@synchronized false
            }
            refreshGroupSummary(manager)
            true
        }
    }

    /** Posts a generic, non-alerting summary so the system can display grouped children. */
    private fun refreshGroupSummary(manager: NotificationManager) {
        val activeChats = activeChatNotifications(manager) ?: return
        val content = TelegramMessageNotificationGrouping.summaryContent(activeChats.size)
        if (content == null) {
            cancelSafely(manager, TelegramMessageNotificationGrouping.SUMMARY_TAG)
            return
        }
        val channelId = activeChats.lastOrNull()?.notification?.channelId ?: PRIVATE_CHANNEL_ID
        val builder = Notification.Builder(appContext, channelId)
            .setSmallIcon(android.R.drawable.sym_action_chat)
            .setContentTitle(content.title)
            .setContentText(content.body)
            .setStyle(Notification.BigTextStyle().bigText(content.body))
            .setCategory(Notification.CATEGORY_MESSAGE)
            .setGroup(TelegramMessageNotificationGrouping.GROUP_KEY)
            .setGroupSummary(true)
            .setGroupAlertBehavior(Notification.GROUP_ALERT_CHILDREN)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(
                Notification.Builder(appContext, channelId)
                    .setSmallIcon(android.R.drawable.sym_action_chat)
                    .setContentTitle(content.title)
                    .setContentText(content.body)
                    .setVisibility(Notification.VISIBILITY_PUBLIC)
                    .build(),
            )
            .setAutoCancel(false)

        summaryPendingIntent()?.let(builder::setContentIntent)
        try {
            manager.notify(
                TelegramMessageNotificationGrouping.SUMMARY_TAG,
                NOTIFICATION_ID,
                builder.build(),
            )
        } catch (_: SecurityException) {
            // A child notification was already delivered; do not turn summary failure into a
            // failed message notification.
        }
    }

    private fun activeChatNotifications(manager: NotificationManager) = runCatching {
        manager.activeNotifications.filter { status ->
            isTelegramMessageNotificationTag(status.tag)
        }
    }.getOrNull()

    private fun summaryPendingIntent(): PendingIntent? {
        val launchIntent = appContext.packageManager
            .getLaunchIntentForPackage(appContext.packageName)
            ?.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            ?: return null
        return PendingIntent.getActivity(
            appContext,
            GROUP_SUMMARY_REQUEST_CODE,
            launchIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    private fun cancelSafely(manager: NotificationManager, tag: String, id: Int = NOTIFICATION_ID) {
        runCatching { manager.cancel(tag, id) }
    }

    private fun ensureChannels() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = notificationManager ?: return
        manager.createNotificationChannel(
            NotificationChannel(
                PRIVATE_CHANNEL_ID,
                "Private chats",
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                description = "New messages in private chats"
                lockscreenVisibility = Notification.VISIBILITY_PRIVATE
            },
        )
        manager.createNotificationChannel(
            NotificationChannel(
                GROUP_CHANNEL_ID,
                "Group chats",
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                description = "New messages in groups"
                lockscreenVisibility = Notification.VISIBILITY_PRIVATE
            },
        )
    }

    private fun hasNotificationPermission(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(
                appContext,
                Manifest.permission.POST_NOTIFICATIONS,
            ) == PackageManager.PERMISSION_GRANTED

    private fun notificationTag(chatId: Long): String = "$TELEGRAM_MESSAGE_NOTIFICATION_TAG_PREFIX$chatId"

    private companion object {
        const val PRIVATE_CHANNEL_ID = "telegram_private_messages"
        const val GROUP_CHANNEL_ID = "telegram_group_messages"
        const val NOTIFICATION_ID = 1
        const val GROUP_SUMMARY_REQUEST_CODE = 0x5447
    }
}

/** Recognizes only the exact tag format emitted for this publisher's chat notifications. */
internal fun isTelegramMessageNotificationTag(tag: String?): Boolean {
    val value = tag?.takeIf { it.startsWith(TELEGRAM_MESSAGE_NOTIFICATION_TAG_PREFIX) }
        ?.removePrefix(TELEGRAM_MESSAGE_NOTIFICATION_TAG_PREFIX)
        ?.toLongOrNull()
        ?: return false
    return value != 0L && tag == "$TELEGRAM_MESSAGE_NOTIFICATION_TAG_PREFIX$value"
}
