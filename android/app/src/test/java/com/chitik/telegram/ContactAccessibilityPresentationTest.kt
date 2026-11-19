package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Test

class ContactAccessibilityPresentationTest {
    @Test
    fun contactLabelIncludesAvailableIdentityAndRelationshipFields() {
        assertEquals(
            "Ada Lovelace, Username @ada_l, Mutual contact",
            contactItemAccessibilityLabel(
                ContactListEntry(
                    userId = 42,
                    displayName = "Ada Lovelace",
                    username = "ada_l",
                    isMutual = true,
                ),
            ),
        )
    }

    @Test
    fun botLabelDoesNotInventUsernameWhenItIsUnavailable() {
        assertEquals(
            "Helper Bot, Not a mutual contact, Bot",
            contactItemAccessibilityLabel(
                ContactListEntry(
                    userId = 7,
                    displayName = "Helper Bot",
                    username = null,
                    isMutual = false,
                    isBot = true,
                ),
            ),
        )
    }

    @Test
    fun publicSearchResultIsLabeledAsATelegramUser() {
        assertEquals(
            "Ada, Username @ada_l, Telegram user",
            contactItemAccessibilityLabel(
                ContactListEntry(
                    userId = 42,
                    displayName = "Ada",
                    username = "ada_l",
                    isMutual = false,
                    isContact = false,
                ),
            ),
        )
    }

    @Test
    fun countLoadingErrorAndLoadMoreLabelsReflectVisibleState() {
        assertEquals("20 of 80 contacts loaded", contactsCountAccessibilityLabel(20, 80))
        assertEquals("Loading contacts", contactsLoadingAccessibilityLabel(0, 80))
        assertEquals(
            "Loading contacts, 20 of 80 contacts loaded",
            contactsLoadingAccessibilityLabel(20, 80),
        )
        assertEquals("Could not load contacts", contactsErrorAccessibilityLabel("  Could not load contacts  "))
        assertEquals(
            "Load more contacts, 20 of 80 contacts loaded",
            contactsLoadMoreAccessibilityLabel(20, 80, isLoading = false),
        )
        assertEquals(
            "Loading contacts, 20 of 80 contacts loaded",
            contactsLoadMoreAccessibilityLabel(20, 80, isLoading = true),
        )
    }
}
