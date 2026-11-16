package com.chitik.telegram

import org.json.JSONObject

/** Parses the canonical Rust contact map without importing any phone-number fields. */
internal fun parseContactListState(root: JSONObject?): List<ContactListEntry> {
    val byUserId = root?.optJSONObject("by_user_id") ?: return emptyList()
    val keys = byUserId.keys().asSequence().toList().sortedBy { it.toLongOrNull() ?: Long.MAX_VALUE }
    return keys.mapNotNull { key ->
        val userId = key.toLongOrNull() ?: return@mapNotNull null
        val contact = byUserId.optJSONObject(key) ?: return@mapNotNull null
        if (userId <= 0L || contact.optLong("user_id") != userId) return@mapNotNull null
        val displayName = contact.optString("display_name").trim()
        if (displayName.isEmpty()) return@mapNotNull null
        ContactListEntry(
            userId = userId,
            displayName = displayName,
            username = contact.optString("username").takeIf(String::isNotEmpty),
            isMutual = contact.optBoolean("is_mutual"),
        )
    }
}

internal data class ContactPage(
    val offset: Int,
    val total: Int,
    val contacts: List<ContactListEntry>,
)

/** Parses a canonical contacts_loaded event, whose contacts are an array. */
internal fun parseContactPage(payload: String): ContactPage? = runCatching {
    val eventPayload = JSONObject(payload).optJSONObject("payload") ?: return null
    val offset = eventPayload.optInt("offset", -1)
    val total = eventPayload.optInt("total", -1)
    val values = eventPayload.optJSONArray("contacts") ?: return null
    if (offset < 0 || total < 0) return null

    val contacts = buildList {
        for (index in 0 until values.length()) {
            val contact = values.optJSONObject(index) ?: continue
            val userId = contact.optLong("user_id")
            val displayName = contact.optString("display_name").trim()
            if (userId <= 0L || displayName.isEmpty()) continue
            add(
                ContactListEntry(
                    userId = userId,
                    displayName = displayName,
                    username = contact.optString("username").takeIf(String::isNotEmpty),
                    isMutual = contact.optBoolean("is_mutual"),
                ),
            )
        }
    }.distinctBy(ContactListEntry::userId).sortedBy(ContactListEntry::userId)
    ContactPage(offset, total, contacts)
}.getOrNull()
