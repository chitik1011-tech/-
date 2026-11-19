package com.chitik.telegram

/** Accessible text derived only from contact fields already supplied by Core. */
internal fun contactItemAccessibilityLabel(contact: ContactListEntry): String = buildList {
    add(contact.displayName)
    contact.username?.takeIf(String::isNotBlank)?.let { add("Username @$it") }
    if (!contact.isContact) add("Telegram user")
    else if (contact.isMutual) add("Mutual contact") else add("Not a mutual contact")
    if (contact.isBot) add("Bot")
}.joinToString(", ")

internal fun contactsCountAccessibilityLabel(loaded: Int, total: Int): String =
    "$loaded of $total contacts loaded"

internal fun contactsLoadingAccessibilityLabel(loaded: Int, total: Int): String =
    if (loaded == 0) "Loading contacts" else "Loading contacts, ${contactsCountAccessibilityLabel(loaded, total)}"

internal fun contactsErrorAccessibilityLabel(error: String): String = error.trim()

internal fun contactsLoadMoreAccessibilityLabel(
    loaded: Int,
    total: Int,
    isLoading: Boolean,
): String = if (isLoading) {
    "Loading contacts, ${contactsCountAccessibilityLabel(loaded, total)}"
} else {
    "Load more contacts, ${contactsCountAccessibilityLabel(loaded, total)}"
}
