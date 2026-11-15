package com.chitik.telegram.deeplink

/** Dedicated Android destination receiving a parsed Telegram link. */
enum class TelegramLinkDestination {
    LinkReview,
}

/** Transient routing state. It is intentionally not serializable or persisted. */
data class PendingTelegramRoute(
    val destination: TelegramLinkDestination,
    val link: TelegramLinkIntent,
)

/** A normalized join request emitted only after an explicit confirmation tap. */
data class TelegramJoinInviteIntent(val inviteHash: String)

object TelegramLinkAppRouter {
    /** Routes a parsed link into the transient link-review destination without network work. */
    fun route(link: TelegramLinkIntent): PendingTelegramRoute =
        PendingTelegramRoute(destination = TelegramLinkDestination.LinkReview, link = link)

    /**
     * Create a join intent only after the caller explicitly confirms the pending invite.
     * This is a typed in-memory command value; it does not execute network I/O.
     */
    fun confirmInvite(route: PendingTelegramRoute): TelegramJoinInviteIntent? {
        if (!route.link.requiresConfirmation) return null
        val invite = route.link.target as? TelegramLinkTarget.Invite ?: return null
        return TelegramJoinInviteIntent(invite.hash)
    }
}