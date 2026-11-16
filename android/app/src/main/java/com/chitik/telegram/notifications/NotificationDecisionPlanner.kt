package com.chitik.telegram.notifications

/**
 * Session-scoped, bounded notification deduplication and lifecycle decisions.
 *
 * A planner instance belongs to one authenticated session. The caller must supply only
 * live incoming updates (never history pages) and discard the planner on logout/re-login.
 * No message contents are retained.
 */
internal class NotificationDecisionPlanner(
    private val capacity: Int = 2048,
) {
    init { require(capacity > 0) }

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
    private var isForeground: Boolean = false

    @Synchronized
    fun reset(session: Long?) {
        seen.clear()
        activeSession = session
        foregroundChat = null
        isForeground = false
    }

    @Synchronized
    fun updateVisibility(foreground: Boolean, chatId: Long?) {
        isForeground = foreground
        foregroundChat = if (foreground) chatId else null
    }

    /**
     * Returns true at most once for an incoming server message within the retained window.
     * A suppressed foreground message is still marked seen to avoid a delayed replay alert.
     */
    @Synchronized
    fun shouldNotify(identity: Identity, outgoing: Boolean, liveUpdate: Boolean): Boolean {
        if (identity.session != activeSession || identity.chatId == 0L ||
            identity.serverMessageId <= 0L || outgoing || !liveUpdate) return false
        if (seen.put(identity, Unit) != null) return false
        return !(isForeground && foregroundChat == identity.chatId)
    }

    @Synchronized
    fun markRead(session: Long, chatId: Long, throughMessageId: Long) {
        if (session != activeSession || throughMessageId <= 0L) return
        // Deliberately retain identities after a read: a reconnect replay must not
        // re-notify a message simply because its notification was dismissed.
        // Read cleanup belongs to the Android publisher; dedup is independent.
    }
}
