package com.chitik.telegram

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AvatarPlaceholderTest {
    @Test
    fun sameDisplayNameAlwaysProducesTheSameStyle() {
        val first = avatarPlaceholderStyle("Ada Lovelace")
        val second = avatarPlaceholderStyle("Ada Lovelace")

        assertEquals(first, second)
        assertEquals("AL", first.initials)
        assertTrue(first.paletteIndex in 0..4)
    }

    @Test
    fun takesUnicodeCodePointsAndHandlesBlankNames() {
        assertEquals("😀猫", avatarPlaceholderStyle("😀 猫").initials)
        assertEquals("?", avatarPlaceholderStyle("   ").initials)
    }
}
