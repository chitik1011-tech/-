package com.chitik.telegram.notifications

/** Filters live JNI message events through the existing parser and session-scoped planner. */
internal class NotificationEventGate(
    private val planner: NotificationDecisionPlanner = NotificationDecisionPlanner(),
) {
    /** Starts a fresh deduplication window for a native session generation. */
    fun reset(sessionGeneration: Long?) = planner.reset(sessionGeneration)

    /** Keeps notification suppression aligned with the visible Android navigation state. */
    fun updateVisibility(foreground: Boolean, chatId: Long?) =
        planner.updateVisibility(foreground, chatId)

    /** Returns validated incoming live message data only when it should be published. */
    fun incoming(
        generation: Long,
        eventType: String,
        payload: String,
        authorized: Boolean,
    ): NotificationWireParser.Incoming? {
        if (!authorized || eventType != "message_added") return null
        val message = NotificationWireParser.parseLiveMessage(payload) ?: return null
        val identity = NotificationDecisionPlanner.Identity(
            session = generation,
            chatId = message.chatId,
            serverMessageId = message.messageId,
        )
        return message.takeIf {
            planner.shouldNotify(identity, outgoing = message.outgoing, liveUpdate = true)
        }
    }
}
