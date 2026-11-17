package com.chitik.telegram

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ContactUiMappingTest {
    @Test
    fun loadedContactSearchMatchesNameAndUsernameWithoutCaseSensitivity() {
        val contacts = listOf(
            ContactListEntry(1, "Ada Lovelace", "ada_l", isMutual = true),
            ContactListEntry(2, "Grace Hopper", "compiler", isMutual = false),
            ContactListEntry(3, "Katherine Johnson", null, isMutual = false),
        )

        assertEquals(listOf(1L), filterLoadedContacts(contacts, "ADA").map(ContactListEntry::userId))
        assertEquals(listOf(2L), filterLoadedContacts(contacts, "@COMP").map(ContactListEntry::userId))
        assertEquals(listOf(3L), filterLoadedContacts(contacts, "johnson").map(ContactListEntry::userId))
    }

    @Test
    fun emptyQueryAndNoMatchesAreHandledWithoutChangingLoadedContacts() {
        val contacts = listOf(
            ContactListEntry(1, "Ada Lovelace", "ada_l", isMutual = true),
            ContactListEntry(2, "Grace Hopper", null, isMutual = false),
        )

        assertEquals(contacts, filterLoadedContacts(contacts, " "))
        assertEquals(contacts, filterLoadedContacts(contacts, ""))
        assertTrue(filterLoadedContacts(contacts, "missing").isEmpty())
    }

    @Test
    fun parsesContactsByStableUserIdAndIgnoresUntrustedFields() {
        val state = JSONObject(
            """{"by_user_id":{
                "42":{"user_id":42,"display_name":"Ada Lovelace","username":"ada_l","is_mutual":true,"is_bot":true,"avatar_photo_id":9001,"phone":"must-not-render"},
                "8":{"user_id":8,"display_name":"Grace Hopper","username":null,"is_mutual":false}
            }}""",
        )

        val contacts = parseContactListState(state)
        assertEquals(listOf(8L, 42L), contacts.map(ContactListEntry::userId))
        assertEquals("ada_l", contacts[1].username)
        assertEquals("Ada Lovelace", contacts[1].displayName)
        assertEquals(true, contacts[1].isBot)
        assertEquals(9001L, contacts[1].avatarPhotoId)
        assertEquals(true, contacts[1].isMutual)
    }

    @Test
    fun rejectsMalformedContactIdentityAndAcceptsMissingBook() {
        val state = JSONObject("""{"by_user_id":{"oops":{"user_id":9,"display_name":"Bad"},"9":{"user_id":10,"display_name":"Mismatch"}}}""")
        assertEquals(emptyList<ContactListEntry>(), parseContactListState(state))
        assertNull(parseContactListState(null).firstOrNull())
    }

    @Test
    fun parsesContactPagesWithoutRetainingRawIdentityFields() {
        val page = parseContactPage(
            """{"type":"contacts_loaded","payload":{"offset":0,"total":1,"contacts":[{"user_id":42,"display_name":"Ada Lovelace","username":"ada_l","is_mutual":true}]}}""",
        )
        assertEquals(0, page?.offset)
        assertEquals(1, page?.total)
        assertEquals(listOf(42L), page?.contacts?.map(ContactListEntry::userId))
        assertNull(parseContactPage("""{"type":"contacts_loaded","payload":{}}"""))
    }
}
