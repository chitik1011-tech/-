package com.chitik.telegram

import org.json.JSONObject

/** Wire projection only; all permission decisions originate in the Rust Telegram mapper. */
data class ChatCapabilitiesUi(
    val canSendNow: Boolean? = null,
    val canSendMessages: Boolean? = null,
    val canSendMedia: Boolean? = null,
    val canSendPolls: Boolean? = null,
    val canSendStickers: Boolean? = null,
    val canPinMessages: Boolean? = null,
    val canDeleteMessages: Boolean? = null,
    val canEditMessages: Boolean? = null,
    val canForwardMessages: Boolean? = null,
    val canInviteUsers: Boolean? = null,
    val canManageChat: Boolean? = null,
    val canPostMessages: Boolean? = null,
    val canEditOthersMessages: Boolean? = null,
    val readOnly: Boolean? = null,
    val restricted: Boolean? = null,
    val banned: Boolean? = null,
    val slowModeEnabled: Boolean? = null,
    val slowModeRemainingSeconds: Long? = null,
)

internal fun parseChatCapabilities(root: JSONObject?): Map<Long, ChatCapabilitiesUi> {
    if (root == null) return emptyMap()
    return buildMap {
        val keys = root.keys()
        while (keys.hasNext()) {
            val key = keys.next()
            val chatId = key.toLongOrNull()?.takeIf { it != 0L } ?: continue
            val value = root.optJSONObject(key) ?: continue
            put(chatId, parseChatCapability(value))
        }
    }
}

private fun parseChatCapability(value: JSONObject) = ChatCapabilitiesUi(
    canSendNow = capabilityBoolean(value, "can_send_now"),
    canSendMessages = capabilityBoolean(value, "can_send_messages"),
    canSendMedia = capabilityBoolean(value, "can_send_media"),
    canSendPolls = capabilityBoolean(value, "can_send_polls"),
    canSendStickers = capabilityBoolean(value, "can_send_stickers"),
    canPinMessages = capabilityBoolean(value, "can_pin_messages"),
    canDeleteMessages = capabilityBoolean(value, "can_delete_messages"),
    canEditMessages = capabilityBoolean(value, "can_edit_messages"),
    canForwardMessages = capabilityBoolean(value, "can_forward_messages"),
    canInviteUsers = capabilityBoolean(value, "can_invite_users"),
    canManageChat = capabilityBoolean(value, "can_manage_chat"),
    canPostMessages = capabilityBoolean(value, "can_post_messages"),
    canEditOthersMessages = capabilityBoolean(value, "can_edit_others_messages"),
    readOnly = capabilityBoolean(value, "read_only"),
    restricted = capabilityBoolean(value, "restricted"),
    banned = capabilityBoolean(value, "banned"),
    slowModeEnabled = capabilityBoolean(value, "slow_mode_enabled"),
    slowModeRemainingSeconds = value.optJSONObject("slow_mode_remaining_seconds")
        ?.takeIf { it.optString("availability") == "known" }
        ?.opt("value")
        ?.let { it as? Number }
        ?.toLong(),
)

private fun capabilityBoolean(root: JSONObject, key: String): Boolean? {
    val capability = root.optJSONObject(key) ?: return null
    if (capability.optString("availability") != "known") return null
    return if (capability.has("value") && !capability.isNull("value")) {
        capability.opt("value") as? Boolean
    } else {
        null
    }
}
