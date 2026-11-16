package com.chitik.telegram.notifications

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.ContextCompat
import java.util.concurrent.atomic.AtomicReference

/**
 * Publishes message notifications from caller-supplied Rust event data.
 *
 * This class performs no Telegram I/O and stores no message or credential data. The Android
 * shell should pass chat/message fields mapped from Rust events and update [setVisibleChat] as
 * navigation changes.
 */
class TelegramNotificationPublisher(context: Context) {
    /** Presentation-only preview policy; never changes Rust domain data. */
    enum class PreviewPolicy { Full, SenderAndChat, Hidden }

    @Volatile
    private var previewPolicy: PreviewPolicy = PreviewPolicy.SenderAndChat

    fun setPreviewPolicy(policy: PreviewPolicy) {
        previewPolicy = policy
    }

    private val appContext = context.applicationContext
    private val notificationManager =
        appContext.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
    private val visibleChatId = AtomicReference<Long?>(null)

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

    /** Dismisses notices for one chat after a read, delete, or navigation event. */
    fun cancelChat(chatId: Long) {
        if (chatId != 0L) notificationManager?.cancel(notificationTag(chatId), NOTIFICATION_ID)
    }

    /** Only clears this application's message notification tags, not other channels. */
    fun cancelAll() {
        notificationManager?.cancelAll()
    }

    /** Marks the currently visible chat; its incoming events are not shown as notifications. */
    fun setVisibleChat(chatId: Long?) {
        visibleChatId.set(chatId)
        if (chatId != null) {
            notificationManager?.cancel(notificationTag(chatId), NOTIFICATION_ID)
        }
    }

    /**
     * Posts a notification unless permission is unavailable or the same chat is currently open.
     * Returns true only when a notification was delivered to Android's NotificationManager.
     */
    fun publish(message: IncomingMessage): Boolean {
        if (message.chatId == 0L || visibleChatId.get() == message.chatId) return false
        if (!hasNotificationPermission()) return false
        val manager = notificationManager ?: return false
        val channelId = if (message.isGroup) GROUP_CHANNEL_ID else PRIVATE_CHANNEL_ID
        val title = if (message.isGroup) {
            message.chatTitle
        } else {
            message.senderName?.takeIf(String::isNotBlank) ?: message.chatTitle
        }.ifBlank { "Telegram" }
        val policy = previewPolicy
        val visibleTitle = if (policy == PreviewPolicy.Hidden) "Telegram" else title
        val body = when (policy) {
            PreviewPolicy.Full -> message.body.ifBlank { "New message" }
            PreviewPolicy.SenderAndChat, PreviewPolicy.Hidden -> "New message"
        }

        val notification = Notification.Builder(appContext, channelId)
            .setSmallIcon(android.R.drawable.sym_action_chat)
            .setContentTitle(visibleTitle)
            .setContentText(body)
            .setStyle(Notification.BigTextStyle().bigText(body))
            .setWhen(message.timestampMillis)
            .setShowWhen(true)
            .setCategory(Notification.CATEGORY_MESSAGE)
            .setGroup(MESSAGE_NOTIFICATION_GROUP)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
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

        return try {
            manager.notify(notificationTag(message.chatId), NOTIFICATION_ID, notification)
            true
        } catch (_: SecurityException) {
            // Permission can be revoked between the check and notify.
            false
        }
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

    private fun notificationTag(chatId: Long): String = "$NOTIFICATION_TAG_PREFIX$chatId"

    private companion object {
        const val PRIVATE_CHANNEL_ID = "telegram_private_messages"
        const val GROUP_CHANNEL_ID = "telegram_group_messages"
        const val NOTIFICATION_TAG_PREFIX = "telegram_chat_"
        const val NOTIFICATION_ID = 1
        const val MESSAGE_NOTIFICATION_GROUP = "telegram_message_chats"
    }
}
