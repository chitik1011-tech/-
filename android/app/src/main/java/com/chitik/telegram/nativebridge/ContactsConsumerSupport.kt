package com.chitik.telegram.nativebridge

import com.chitik.telegram.ContactListEntry
import com.chitik.telegram.parseContactPage

/** Applies one live contacts page to the currently displayed contacts state. */
internal fun NativeUiState.reduceContactsLoaded(payload: String): NativeUiState {
    val page = parseContactPage(payload) ?: return copy(
        contactsLoading = false,
        error = "Invalid contacts response",
    )
    val updatedContacts = if (page.offset == 0) {
        page.contacts
    } else {
        (contacts + page.contacts).distinctBy(ContactListEntry::userId)
            .sortedBy(ContactListEntry::userId)
    }
    return copy(
        contacts = updatedContacts,
        contactsLoading = false,
        contactTotal = page.total,
        lastEventType = "contacts_loaded",
        // Keep contact names and usernames out of diagnostic state.
        lastEventPayload = "{\"offset\":${page.offset},\"count\":${page.contacts.size},\"total\":${page.total}}",
        error = null,
    )
}

/** Describes the first page requested after the native core asks Android to refresh contacts. */
internal data class ContactsRefreshRequest(val offset: Int = 0, val limit: Int = 100)

internal fun contactsRefreshRequestForEvent(eventType: String): ContactsRefreshRequest? =
    if (eventType == "contacts_refresh_requested") ContactsRefreshRequest() else null
