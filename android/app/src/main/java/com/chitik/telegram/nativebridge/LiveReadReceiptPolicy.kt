package com.chitik.telegram.nativebridge

/** Selects validated live incoming messages that should advance a visible chat's read cursor. */
internal class LiveReadReceiptPolicy {
    private var sessionGeneration: Long? = null
    private var foregroundChatId: Long? = null
    private var foreground = false
    private val highestReadMessageByChat = mutableMapOf<Long, Long>()

    @Synchronized
    fun reset(generation: Long?) {
        sessionGeneration = generation
        if (generation == null) {
            foregroundChatId = null
            foreground = false
        }
        highestReadMessageByChat.clear()
    }

    @Synchronized
    fun updateVisibility(isForeground: Boolean, chatId: Long?) {
        foreground = isForeground
        foregroundChatId = chatId?.takeIf { isForeground && it != 0L }
    }

    /** Returns the new monotonic read cursor, or null when the event must not be acknowledged. */
    @Synchronized
    fun onIncomingMessage(
        generation: Long,
        chatId: Long,
        messageId: Long,
        outgoing: Boolean,
        authorized: Boolean,
    ): Long? {
        if (
            !authorized || outgoing || generation != sessionGeneration ||
            !foreground || chatId == 0L || chatId != foregroundChatId || messageId <= 0L
        ) return null
        val previous = highestReadMessageByChat[chatId] ?: 0L
        if (messageId <= previous) return null
        highestReadMessageByChat[chatId] = messageId
        return messageId
    }
}
