package com.chitik.telegram

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MessageMutationSupportTest {
    @Test
    fun editAndDeleteStayUnavailableUntilBackendCommandsExist() {
        val edit = messageMutationAvailability(MessageMutationAction.Edit)
        val delete = messageMutationAvailability(MessageMutationAction.Delete)

        assertFalse(edit.enabled)
        assertFalse(delete.enabled)
        assertTrue(edit.explanation.contains("не поддерживается"))
        assertTrue(delete.explanation.contains("не поддерживается"))
    }
}
