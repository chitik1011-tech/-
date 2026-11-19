package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PhoneContactSyncTest {
    @Test
    fun acceptsOnlyNormalizedInternationalNumbers() {
        assertEquals("+12025550123", normalizeE164Candidate("202-555-0123", "+12025550123"))
        assertEquals("+442071838750", normalizeE164Candidate("+44 20 7183 8750", null))
        assertNull(normalizeE164Candidate("202-555-0123", null))
        assertNull(normalizeE164Candidate("+01234567", null))
        assertNull(normalizeE164Candidate("+123", null))
    }

    @Test
    fun splitsDisplayNamesWithoutProducingBlankFirstNames() {
        assertEquals("Ada" to "Lovelace", splitPhoneContactName(" Ada Lovelace "))
        assertEquals("Madonna" to "", splitPhoneContactName("Madonna"))
        assertEquals("Contact" to "", splitPhoneContactName("  "))
    }
}
