package com.chitik.telegram.notifications

/** Session-scoped bounded deduplication and foreground-suppression decisions. */
internal class NotificationDecisionPlanner(
    private val capacity: Int = 2048,
) {
    init {
        require(capacity > 0) { "capacity must be positive" }
    }

    data class Identity(
        val session: Long,
        val chatId: Long,
        val serverMessageId: Long,
    )

    private val seen = object : LinkedHashMap<Identity, Unit>(capacity.coerceAtMost(128), 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<Identity, Unit>): Boolean =
            size > capacity
    }
    private var activeSession: Long? = null
    private var foregroundChat: Long? = null
    private var isForeground = false

    /** Clears deduplication and visibility state when the authenticated session changes. */
    @Synchronized
    fun reset(session: Long?) {
        seen.clear()
        activeSession = session
        foregroundChat = null
        isForeground = false
    }

    /** Updates the visible-chat context used to suppress notifications for that same chat. */
    @Synchronized
    fun updateVisibility(foreground: Boolean, chatId: Long?) {
        isForeground = foreground
        foregroundChat = if (foreground) chatId else null
    }

    /**
     * Returns true once per valid incoming live message. Messages suppressed because their chat
     * is already open are still recorded, so reconnect replays cannot show them later.
     */
    @Synchronized
    fun shouldNotify(identity: Identity, outgoing: Boolean, liveUpdate: Boolean): Boolean {
        if (
            identity.session != activeSession ||
            identity.chatId == 0L ||
            identity.serverMessageId <= 0L ||
            outgoing ||
            !liveUpdate
        ) {
            return false
        }
        if (seen.put(identity, Unit) != null) return false
        return !(isForeground && foregroundChat == identity.chatId)
    }
}
