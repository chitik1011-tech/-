package com.chitik.telegram.nativebridge

import com.chitik.telegram.ContactListEntry
import com.chitik.telegram.parseContactPage
import org.json.JSONObject

/** Applies one live contacts page to the currently displayed contacts state. */
internal fun NativeUiState.reduceContactsLoaded(payload: String): NativeUiState {
    val requestId = runCatching {
        JSONObject(payload).optJSONObject("payload")?.optLong("request_id", 0L)
            ?.takeIf { it > 0L }
    }.getOrNull()
    val page = parseContactPage(payload) ?: return if (
        contactsLoading && requestId == contactsRequestId
    ) {
        copy(contactsLoading = false, error = "Invalid contacts response")
    } else {
        this
    }
    if (!contactsLoading || page.requestId != contactsRequestId) return this
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
        contactsSearchQuery = page.query,
        lastEventType = "contacts_loaded",
        // Keep contact names and usernames out of diagnostic state.
        lastEventPayload = "{\"offset\":${page.offset},\"count\":${page.contacts.size},\"total\":${page.total}}",
        error = null,
    )
}

internal fun NativeUiState.reduceContactsLoadFailed(payload: String): NativeUiState {
    val eventPayload = runCatching {
        JSONObject(payload).optJSONObject("payload")
    }.getOrNull() ?: return this
    val requestId = eventPayload.optLong("request_id", 0L).takeIf { it > 0L }
    if (!contactsLoading || requestId != contactsRequestId) return this
    val message = eventPayload.optString("error").ifBlank { "Could not load contacts" }
    return copy(
        contactsLoading = false,
        lastEventType = "contacts_load_failed",
        lastEventPayload = "{\"request_id\":$requestId}",
        error = message,
    )
}

/** Describes the first page requested after the native core asks Android to refresh contacts. */
internal data class ContactsRefreshRequest(val offset: Int = 0, val limit: Int = 100)

internal fun contactsRefreshRequestForEvent(eventType: String): ContactsRefreshRequest? =
    if (eventType == "contacts_refresh_requested") ContactsRefreshRequest() else null
