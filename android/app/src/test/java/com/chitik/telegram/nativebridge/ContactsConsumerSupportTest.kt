package com.chitik.telegram.nativebridge

import com.chitik.telegram.ContactListEntry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ContactsConsumerSupportTest {
    @Test
    fun firstPageReplacesExistingContactsAndUpdatesTotal() {
        val current = NativeUiState(
            contacts = listOf(contact(1L), contact(2L)),
            contactTotal = 10,
            contactsLoading = true,
            contactsRequestId = 5L,
        )

        val updated = current.reduceContactsLoaded(
            page(offset = 0, total = 3, contacts = """[{"user_id":8,"display_name":"New"}]""", requestId = 5L),
        )

        assertEquals(listOf(8L), updated.contacts.map(ContactListEntry::userId))
        assertEquals(3, updated.contactTotal)
        assertFalse(updated.contactsLoading)
        assertNull(updated.error)
    }

    @Test
    fun laterPageAppendsDeduplicatesAndUpdatesTotal() {
        val current = NativeUiState(
            contacts = listOf(contact(2L), contact(8L)),
            contactTotal = 8,
            contactsLoading = true,
            contactsRequestId = 6L,
        )

        val updated = current.reduceContactsLoaded(
            page(
                offset = 2,
                total = 9,
                contacts = """[{"user_id":8,"display_name":"Duplicate"},{"user_id":5,"display_name":"New"}]""",
                requestId = 6L,
            ),
        )

        assertEquals(listOf(2L, 5L, 8L), updated.contacts.map(ContactListEntry::userId))
        assertEquals("Contact 8", updated.contacts.last().displayName)
        assertEquals(9, updated.contactTotal)
    }

    @Test
    fun malformedPageStopsLoadingAndPreservesLoadedContactsAndTotal() {
        val existing = listOf(contact(4L))
        val current = NativeUiState(
            contacts = existing,
            contactTotal = 7,
            contactsLoading = true,
        )

        val updated = current.reduceContactsLoaded("not-json")

        assertEquals(existing, updated.contacts)
        assertEquals(7, updated.contactTotal)
        assertFalse(updated.contactsLoading)
        assertEquals("Invalid contacts response", updated.error)
    }

    @Test
    fun lateContactSearchSuccessAndFailureCannotReplaceNewerResults() {
        val current = NativeUiState(
            contacts = listOf(contact(2L)),
            contactTotal = 1,
            contactsLoading = true,
            contactsRequestId = 12L,
            contactsSearchQuery = "grace",
        )
        val latePage = page(
            offset = 0,
            total = 1,
            contacts = """[{"user_id":1,"display_name":"Ada"}]""",
            requestId = 11L,
            query = "ada",
        )

        assertEquals(current, current.reduceContactsLoaded(latePage))
        assertEquals(
            current,
            current.reduceContactsLoadFailed(
                """{"type":"contacts_load_failed","payload":{"request_id":11,"offset":0,"query":"ada","error":"stale"}}""",
            ),
        )

        val matching = current.reduceContactsLoaded(
            page(
                offset = 0,
                total = 1,
                contacts = """[{"user_id":3,"display_name":"Grace","is_contact":false}]""",
                requestId = 12L,
                query = "grace",
            ),
        )
        assertEquals(listOf(3L), matching.contacts.map(ContactListEntry::userId))
        assertEquals(false, matching.contacts.single().isContact)
        assertEquals("grace", matching.contactsSearchQuery)
        assertFalse(matching.contactsLoading)

        val failed = current.reduceContactsLoadFailed(
            """{"type":"contacts_load_failed","payload":{"request_id":12,"offset":0,"query":"grace","error":"Telegram unavailable"}}""",
        )
        assertEquals("Telegram unavailable", failed.error)
        assertFalse(failed.contactsLoading)
        assertEquals(listOf(2L), failed.contacts.map(ContactListEntry::userId))
    }

    @Test
    fun contactsRefreshRequestsTheFirstPage() {
        assertEquals(
            ContactsRefreshRequest(offset = 0, limit = 100),
            contactsRefreshRequestForEvent("contacts_refresh_requested"),
        )
        assertNull(contactsRefreshRequestForEvent("contacts_loaded"))
    }

    private fun contact(id: Long) =
        ContactListEntry(id, "Contact $id", null, isMutual = false)

    private fun page(
        offset: Int,
        total: Int,
        contacts: String,
        requestId: Long? = 5L,
        query: String? = null,
    ): String {
        val id = requestId?.let { "\"request_id\":$it," }.orEmpty()
        val q = query?.let { "\"query\":\"$it\"," }.orEmpty()
        return """{"type":"contacts_loaded","payload":{"offset":$offset,"total":$total,$id$q"contacts":$contacts}}"""
    }
}
