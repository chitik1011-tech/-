package com.chitik.telegram

/** Real destinations/actions available from the chat-list navigation drawer. */
internal enum class ChatListDrawerAction {
    CHATS,
    CONTACTS,
    LOG_OUT,
}

/** Closes the drawer before dispatching the action selected by the user. */
internal suspend fun performChatListDrawerAction(
    action: ChatListDrawerAction,
    closeDrawer: suspend () -> Unit,
    onOpenContacts: () -> Unit,
    onRequestLogout: () -> Unit,
) {
    closeDrawer()
    when (action) {
        ChatListDrawerAction.CHATS -> Unit
        ChatListDrawerAction.CONTACTS -> onOpenContacts()
        ChatListDrawerAction.LOG_OUT -> onRequestLogout()
    }
}
