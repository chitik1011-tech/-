package com.chitik.telegram

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ContactUiMappingTest {
    @Test
    fun parsesContactsByStableUserIdAndIgnoresUntrustedFields() {
        val state = JSONObject(
            """{"by_user_id":{
                "42":{"user_id":42,"display_name":"Ada Lovelace","username":"ada_l","is_mutual":true,"is_bot":true,"phone":"must-not-render"},
                "8":{"user_id":8,"display_name":"Grace Hopper","username":null,"is_mutual":false}
            }}""",
        )

        val contacts = parseContactListState(state)
        assertEquals(listOf(8L, 42L), contacts.map(ContactListEntry::userId))
        assertEquals("ada_l", contacts[1].username)
        assertEquals("Ada Lovelace", contacts[1].displayName)
        assertEquals(true, contacts[1].isBot)
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
