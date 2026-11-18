package com.chitik.telegram

import com.chitik.telegram.nativebridge.GLOBAL_MESSAGE_SEARCH_LIMIT
import com.chitik.telegram.nativebridge.GLOBAL_MESSAGE_SEARCH_SCOPE
import com.chitik.telegram.nativebridge.globalMessageSearchCommand
import com.chitik.telegram.nativebridge.parseGlobalMessageSearchResults
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class GlobalMessageSearchTest {
    @Test
    fun commandUsesExistingSearchMessagesWithAccountWideScope() {
        val command = JSONObject(globalMessageSearchCommand(9L, "  hello  "))
        val payload = command.getJSONObject("payload")

        assertEquals("search_messages", command.getString("type"))
        assertEquals(9L, payload.getLong("request_id"))
        assertEquals(GLOBAL_MESSAGE_SEARCH_SCOPE, payload.getLong("chat_id"))
        assertEquals("hello", payload.getString("query"))
        assertEquals(GLOBAL_MESSAGE_SEARCH_LIMIT, payload.getInt("limit"))
    }

    @Test
    fun parsesGlobalResultsWithTheirRealChatIdsAndRejectsMissingChatIds() {
        val payload = """{"type":"message_search_completed","payload":{"request_id":9,"chat_id":0,"messages":[{"id":45,"chat_id":771,"sender_id":4,"text":"hello world","timestamp_unix":1700000000},{"id":46,"chat_id":0,"text":"invalid"}]}}"""
        val parsed = parseGlobalMessageSearchResults(payload)

        assertNotNull(parsed)
        assertEquals(9L, parsed!!.first)
        assertEquals(1, parsed.second.size)
        assertEquals(771L, parsed.second.single().chatId)
        assertEquals(45L, parsed.second.single().messageId)
        assertEquals("hello world", parsed.second.single().text)
    }

    @Test
    fun ignoresMalformedSearchPayloads() {
        assertNull(parseGlobalMessageSearchResults("not json"))
        assertNull(parseGlobalMessageSearchResults("{}"))
    }
}
