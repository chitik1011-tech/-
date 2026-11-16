package com.chitik.telegram.nativebridge

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.chitik.telegram.AuthorizationUiStep
import com.chitik.telegram.ChatListEntry
import com.chitik.telegram.ChatListPreview
import com.chitik.telegram.ChatCapabilitiesUi
import com.chitik.telegram.parseChatCapabilities
import com.chitik.telegram.ConversationMessageUi
import com.chitik.telegram.ConversationReactionUi
import com.chitik.telegram.ReactionRevisionUi
import com.chitik.telegram.ConversationLocationUi
import com.chitik.telegram.ConversationGeoPointUi
import com.chitik.telegram.ContactListEntry
import com.chitik.telegram.parseContactListState
import com.chitik.telegram.parseContactPage
import com.chitik.telegram.ConversationPollOptionUi
import com.chitik.telegram.ConversationPollUi
import com.chitik.telegram.PeerProfileUi
import com.chitik.telegram.MediaAttachmentUi
import com.chitik.telegram.matchesMutationTarget
import com.chitik.telegram.mutationEventAcknowledges
import com.chitik.telegram.messageMutationStateAfterSessionChange
import com.chitik.telegram.mediaDownloadStateFromWire
import com.chitik.telegram.mediaKindFromWire
import com.chitik.telegram.parseServiceAction
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

data class NativeUiState(
    val libraryLoaded: Boolean = false,
    val coreVersion: String = "Not loaded",
    val nativeHandle: Long = 0L,
    val telegramConfigured: Boolean = false,
    val authorization: AuthorizationUiStep = AuthorizationUiStep.Closed,
    val chats: List<ChatListEntry> = emptyList(),
    val profiles: Map<Long, PeerProfileUi> = emptyMap(),
    val profileLoadingPeerId: Long? = null,
    val profileError: String? = null,
    val contacts: List<ContactListEntry> = emptyList(),
    val contactTotal: Int = 0,
    val contactsLoading: Boolean = false,
    val chatCapabilities: Map<Long, ChatCapabilitiesUi> = emptyMap(),
    val messages: Map<Long, List<ConversationMessageUi>> = emptyMap(),
    val pendingReactionKeys: Set<String> = emptySet(),
    val reactionConnectionToken: Pair<Long, Long>? = null,
    val reactionErrors: Map<String, String> = emptyMap(),
    val votingPollIds: Set<Long> = emptySet(),
    val pollVoteErrors: Map<Long, String> = emptyMap(),
    val pinnedMessages: Map<Long, List<Long>> = emptyMap(),
    val pendingPinnedMessageIds: Set<String> = emptySet(),
    val pinnedError: String? = null,
    val pinnedFailureKey: String? = null,
    val loadingHistoryBefore: Map<Long, Long> = emptyMap(),
    val completedHistoryBefore: Map<Long, Long> = emptyMap(),
    val historyPageErrors: Map<Long, String> = emptyMap(),
    val forwardError: String? = null,
    val forwardSuccessCount: Long = 0,
    val messageMutationPending: Boolean = false,
    val messageMutationError: String? = null,
    val messageMutationChatId: Long = 0L,
    val messageMutationMessageId: Long = 0L,
    val messageMutationSuccessCount: Long = 0L,
    val messageMutationSuccessChatId: Long = 0L,
    val messageMutationSuccessMessageId: Long = 0L,
    val selectionGeneration: Long = 0,
    val lastConnectionSessionGeneration: Long? = null,
    val deletingMessages: Boolean = false,
    val outbox: Map<Long, OutgoingTextSendUi> = emptyMap(),
    val activeTextSendId: Long? = null,
    val lastTextSendId: Long? = null,
    val activeTextSendChatId: Long? = null,
    val activeTextSendText: String? = null,
    val failedTextSendId: Long? = null,
    val failedTextSendText: String? = null,
    val textSendError: String? = null,
    val deleteError: String? = null,
    val busy: Boolean = false,
    val lastEventType: String = "none",
    val lastEventPayload: String = "",
    val error: String? = null,
) {
    val sendingText: Boolean
        get() = activeTextSendId != null &&
            (outbox[activeTextSendId]?.isInFlight ?: true)

    val coreReady: Boolean
        get() = libraryLoaded && nativeHandle != 0L && error == null
}

internal fun NativeUiState.applyReactionUpdate(
    chatId: Long,
    messageId: Long,
    reactions: List<ConversationReactionUi>,
    revision: ReactionRevisionUi,
): NativeUiState {
    val chatMessages = messages[chatId] ?: return this
    val index = chatMessages.indexOfFirst { it.id == messageId.toString() }
    if (index < 0) return this
    val current = chatMessages[index]
    if (current.reactionRevision?.let { revision <= it } == true) return this

    val updatedMessages = chatMessages.toMutableList()
    updatedMessages[index] = current.copy(reactions = reactions, reactionRevision = revision)
    val key = "$chatId:$messageId"
    return copy(
        messages = messages + (chatId to updatedMessages),
        pendingReactionKeys = pendingReactionKeys - key,
        reactionErrors = reactionErrors - key,
    )
}

internal fun NativeUiState.applyConnectionLifecycle(payload: String): NativeUiState {
    val body = runCatching { JSONObject(payload).optJSONObject("payload") }.getOrNull()
        ?: return this
    val token = body.optJSONObject("token")?.let { tokenJson ->
        val generation = tokenJson.optLong("generation", -1L)
        val epoch = tokenJson.optLong("epoch", -1L)
        if (generation < 0L || epoch < 0L) null else generation to epoch
    }
    val event = body.optJSONObject("event")
    val eventType = event?.optString("type")?.takeIf(String::isNotBlank)
        ?: event?.keys()?.asSequence()?.firstOrNull()
    val disconnected = eventType in setOf(
        "transport_lost",
        "connect_failed",
        "retry",
        "logout",
        "shutdown",
    )
    val generationChanged = token != null && reactionConnectionToken != null &&
        token != reactionConnectionToken
    val updated = copy(reactionConnectionToken = token ?: reactionConnectionToken)
    return if (disconnected || generationChanged) {
        updated.clearPendingReactionsAfterConnectionChange()
    } else {
        updated
    }
}

internal fun NativeUiState.applyConnectionLifecycleAndGeneration(payload: String): NativeUiState {
    val updated = applyConnectionLifecycle(payload)
    val generation = connectionSessionGenerationFromEvent(payload)
    return generation?.let(updated::recordConnectionSessionGeneration) ?: updated
}

internal fun NativeUiState.finishBulkDelete(
    chatId: Long?,
    ids: Set<Long>,
): NativeUiState = removeDeletedMessages(chatId, ids).copy(
    busy = false,
    deletingMessages = false,
    deleteError = null,
)

internal fun NativeUiState.clearPendingReactionsAfterConnectionChange(): NativeUiState {
    if (pendingReactionKeys.isEmpty()) return this
    val message = "Результат реакции не подтверждён после смены соединения"
    return copy(
        pendingReactionKeys = emptySet(),
        reactionErrors = reactionErrors + pendingReactionKeys.associateWith { message },
    )
}

internal fun NativeUiState.removeDeletedMessages(
    chatId: Long?,
    messageIds: Set<Long>,
): NativeUiState {
    if (messageIds.isEmpty()) return this
    fun isDeleted(chatKey: String): Boolean {
        val parts = chatKey.split(':', limit = 2)
        if (parts.size != 2) return false
        val keyChat = parts[0].toLongOrNull() ?: return false
        val keyMessage = parts[1].toLongOrNull() ?: return false
        return keyMessage in messageIds && (chatId == null || chatId == keyChat)
    }

    val updatedMessages = messages.mapValues { (id, chatMessages) ->
        if (chatId != null && id != chatId) chatMessages
        else chatMessages.filterNot { it.id.toLongOrNull()?.let(messageIds::contains) == true }
    }.filterValues { it.isNotEmpty() }
    val deletedReactionKeys = (pendingReactionKeys + reactionErrors.keys).filter(::isDeleted).toSet()
    return copy(
        messages = updatedMessages,
        pendingReactionKeys = pendingReactionKeys - deletedReactionKeys,
        reactionErrors = reactionErrors - deletedReactionKeys,
    )
}

internal fun shouldResolvePinnedAction(pts: Int?, chatId: Long, messageIds: List<Long>): Boolean =
    pts == null && chatId != 0L && messageIds.isNotEmpty()

internal fun shouldClearPinnedFailure(failureKey: String?, chatId: Long, messageIds: List<Long>, pts: Int?): Boolean =
    shouldResolvePinnedAction(pts, chatId, messageIds) &&
        failureKey != null && messageIds.any { "$chatId:$it" == failureKey }

private data class ParsedNativeState(
    val authorization: AuthorizationUiStep,
    val chats: List<ChatListEntry>,
    val messages: Map<Long, List<ConversationMessageUi>>,
    val pinnedMessages: Map<Long, List<Long>>,
    val contacts: List<ContactListEntry>,
    val profiles: Map<Long, PeerProfileUi>,
    val chatCapabilities: Map<Long, ChatCapabilitiesUi>,
    val outbox: Map<Long, OutgoingTextSendUi>,
)

internal fun NativeUiState.advanceSelectionGeneration(): NativeUiState = copy(
    selectionGeneration = selectionGeneration + 1,
    deletingMessages = false,
    activeTextSendId = null,
    lastTextSendId = null,
    activeTextSendChatId = null,
    activeTextSendText = null,
    outbox = emptyMap(),
    failedTextSendId = activeTextSendId ?: failedTextSendId,
    failedTextSendText = activeTextSendText ?: failedTextSendText,
    textSendError = if (activeTextSendId != null) "Сессия сменилась: проверьте, доставлено ли сообщение, перед повтором" else textSendError,
    pinnedMessages = emptyMap(),
    pendingPinnedMessageIds = emptySet(),
    pinnedError = null,
    pinnedFailureKey = null,
    loadingHistoryBefore = emptyMap(),
    completedHistoryBefore = emptyMap(),
    historyPageErrors = emptyMap(),
    deleteError = if (deletingMessages) {
        "Удаление прервано; обновите сообщения и проверьте результат"
    } else {
        deleteError
    },
)

internal fun NativeUiState.recordConnectionSessionGeneration(generation: Long): NativeUiState =
    when (lastConnectionSessionGeneration) {
        null -> copy(lastConnectionSessionGeneration = generation)
        generation -> this
        else -> copy(lastConnectionSessionGeneration = generation).advanceSelectionGeneration()
    }

internal fun connectionSessionGenerationFromEvent(payload: String): Long? = runCatching {
    val eventPayload = JSONObject(payload).optJSONObject("payload") ?: return@runCatching null
    val token = eventPayload.optJSONObject("token") ?: return@runCatching null
    token.optLong("generation", -1L).takeIf { it >= 0L }
}.getOrNull()

class NativeSessionViewModel(application: Application) : AndroidViewModel(application) {

    private val _state = MutableStateFlow(NativeUiState())
    val state: StateFlow<NativeUiState> = _state.asStateFlow()

    private val lifecycle = NativeSessionLifecycle()

    init {
        initializeNativeSession()
    }

    private fun initializeNativeSession() {
        if (!NativeLibrary.isLoaded) {
            lifecycle.runIfOpen {
                _state.value = NativeUiState(
                    libraryLoaded = false,
                    error = NativeLibrary.errorMessage ?: "Native library is unavailable",
                )
            }
            return
        }

        _state.update { it.copy(libraryLoaded = true, busy = true, error = null) }

        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val version = NativeBridge.coreVersion()
                val config = TelegramRuntimeConfigStore.load(getApplication())
                val newHandle = replaceNativeHandle(config)
                Triple(version, newHandle, config != null)
            }.onSuccess { (version, newHandle, configured) ->
                lifecycle.runIfOpen {
                    _state.update {
                        it.copy(
                            libraryLoaded = true,
                            coreVersion = version,
                            nativeHandle = newHandle,
                            telegramConfigured = configured,
                            busy = false,
                            error = null,
                        )
                    }
                }
                if (configured) {
                    sendStartCommand()
                }
            }.onFailure { error ->
                lifecycle.runIfOpen {
                    _state.update {
                        it.copy(
                            libraryLoaded = true,
                            nativeHandle = lifecycle.currentHandleOrZero(),
                            busy = false,
                            error = error.message ?: error::class.java.simpleName,
                        )
                    }
                }
            }
        }
    }

    fun saveTelegramConfig(apiId: String, apiHash: String) {
        _state.update { it.copy(busy = true, error = null) }

        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val config = TelegramRuntimeConfigStore.save(
                    getApplication(),
                    apiId,
                    apiHash,
                )
                replaceWithTelegramHandle(config)
            }.onSuccess { newHandle ->
                val updated = lifecycle.runIfOpen {
                    _state.update {
                        it.copy(
                            nativeHandle = newHandle,
                            telegramConfigured = true,
                            authorization = AuthorizationUiStep.Closed,
                            chats = emptyList(),
                            messages = emptyMap(),
                            votingPollIds = emptySet(),
                            pollVoteErrors = emptyMap(),
                            busy = false,
                            error = null,
                        )
                    }
                }
                if (updated) {
                    sendStartCommand()
                }
            }.onFailure { error ->
                lifecycle.runIfOpen {
                    _state.update {
                        it.copy(
                            nativeHandle = lifecycle.currentHandleOrZero(),
                            busy = false,
                            error = error.message ?: error::class.java.simpleName,
                        )
                    }
                }
            }
        }
    }

    fun logout() {
        if (_state.value.authorization != AuthorizationUiStep.Ready || _state.value.busy) return
        _state.update { it.copy(busy = true, error = null) }
        if (!sendCommand(JSONObject().put("type", "logout").toString())) {
            _state.update { it.copy(busy = false) }
        }
    }

    fun restartAuthorization() {
        _state.update { it.copy(busy = true, error = null) }

        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val config = TelegramRuntimeConfigStore.load(getApplication())
                config to replaceNativeHandle(config)
            }.onSuccess { (config, newHandle) ->
                val updated = lifecycle.runIfOpen {
                    _state.update {
                        it.copy(
                            nativeHandle = newHandle,
                            telegramConfigured = config != null,
                            authorization = AuthorizationUiStep.Closed,
                            chats = emptyList(),
                            messages = emptyMap(),
                            votingPollIds = emptySet(),
                            pollVoteErrors = emptyMap(),
                            busy = false,
                            error = null,
                        )
                    }
                }
                if (updated && config != null) {
                    sendStartCommand()
                }
            }.onFailure { error ->
                lifecycle.runIfOpen {
                    _state.update {
                        it.copy(
                            nativeHandle = lifecycle.currentHandleOrZero(),
                            busy = false,
                            error = error.message ?: error::class.java.simpleName,
                        )
                    }
                }
            }
        }
    }

    private fun replaceWithTelegramHandle(config: TelegramRuntimeConfig): Long =
        replaceNativeHandle(config)

    private fun replaceNativeHandle(config: TelegramRuntimeConfig?): Long {
        markMessageMutationUnconfirmed("Сессия изменилась; проверьте сообщение в Telegram перед повтором")
        val creation = lifecycle.beginReplacement()
            ?: throw IllegalStateException("Native session replacement is already in progress")

        var createdHandle = 0L
        try {
            creation.detachedHandle?.let { oldHandle ->
                if (NativeLibrary.isLoaded) {
                    NativeBridge.destroyHandle(oldHandle)
                }
            }

            val callback = object : NativeEventCallback {
                override fun onNativeEvent(type: String, payload: String) {
                    this@NativeSessionViewModel.onNativeEvent(
                        creation.generation,
                        type,
                        payload,
                    )
                }
            }

            createdHandle = if (config != null) {
                NativeBridge.createTelegramHandle(
                    callback,
                    config.apiId,
                    config.apiHash,
                    config.sessionDatabasePath,
                )
            } else {
                NativeBridge.createHandle(callback)
            }
            require(createdHandle != 0L) { "Rust returned an invalid native handle" }

            NativeBridge.probe(createdHandle)

            if (!lifecycle.installHandle(creation.generation, createdHandle)) {
                NativeBridge.destroyHandle(createdHandle)
                createdHandle = 0L
                throw IllegalStateException("Native session owner was already cleared")
            }

            return createdHandle
        } catch (error: Throwable) {
            lifecycle.failCreation(creation.generation)
            if (createdHandle != 0L && lifecycle.currentHandleOrZero() != createdHandle) {
                runCatching { NativeBridge.destroyHandle(createdHandle) }
            }
            throw error
        }
    }

    fun probeRoundTrip() {
        if (!NativeLibrary.isLoaded) {
            setErrorIfOpen("Native bridge is not available")
            return
        }

        val result = runCatching {
            lifecycle.dispatchCommand { currentHandle ->
                NativeBridge.probe(currentHandle)
            }
        }.getOrElse { error ->
            setErrorIfOpen(error.message ?: error::class.java.simpleName)
            return
        }

        reportDispatchResult(result, "Rust rejected the JNI probe")
    }

    fun sendStartCommand() {
        if (!lifecycle.runIfOpen {
                _state.update { it.copy(busy = true, error = null) }
            }
        ) {
            return
        }
        val commandJson = JSONObject().put("type", "start").toString()
        val result = runCatching {
            lifecycle.dispatchStart { currentHandle ->
                NativeBridge.sendCommand(currentHandle, commandJson)
            }
        }.getOrElse { error ->
            setErrorIfOpen(error.message ?: error::class.java.simpleName)
            return
        }

        if (result == LifecycleDispatchResult.Duplicate) {
            _state.update { it.copy(busy = false) }
        } else {
            reportDispatchResult(result, "Rust rejected the command")
        }
    }

    fun submitPhoneNumber(phone: String) {
        _state.update { it.copy(busy = true, error = null) }
        sendCommand(
            JSONObject()
                .put("type", "set_phone_number")
                .put("payload", phone.trim())
                .toString(),
        )
    }

    fun submitCode(code: String) {
        _state.update { it.copy(busy = true, error = null) }
        sendCommand(
            JSONObject()
                .put("type", "submit_code")
                .put("payload", code.trim())
                .toString(),
        )
    }

    fun submitPassword(password: String) {
        _state.update { it.copy(busy = true, error = null) }
        sendCommand(
            JSONObject()
                .put("type", "submit_password")
                .put("payload", password)
                .toString(),
        )
    }

    fun loadChats(limit: Int = 100) {
        _state.update { it.copy(busy = true, error = null) }
        val commandJson = JSONObject()
            .put("type", "load_chats")
            .put("payload", JSONObject().put("limit", limit))
            .toString()

        val result = runCatching {
            lifecycle.dispatchLoadChats { currentHandle ->
                NativeBridge.sendCommand(currentHandle, commandJson)
            }
        }.getOrElse { error ->
            setErrorIfOpen(error.message ?: error::class.java.simpleName)
            return
        }

        reportDispatchResult(result, "Rust rejected the chat request")
    }

    fun loadProfile(peerId: Long) {
        if (peerId == 0L) return
        _state.update { it.copy(profileLoadingPeerId = peerId, profileError = null) }
        val accepted = sendCommand(
            JSONObject()
                .put("type", "load_profile")
                .put("payload", JSONObject().put("peer_id", peerId))
                .toString(),
        )
        if (!accepted) {
            _state.update {
                it.copy(
                    profileLoadingPeerId = null,
                    profileError = it.error ?: "Rust rejected the profile request",
                )
            }
        }
    }

/* loadMessagesBefore is shared by reply and pinned navigation below. */
    fun loadMessages(chatId: Long, limit: Int = 100) {
        _state.update { it.copy(busy = true, error = null, deleteError = null) }
        sendCommand(
            JSONObject()
                .put("type", "load_messages")
                .put(
                    "payload",
                    JSONObject()
                        .put("chat_id", chatId)
                        .put("limit", limit),
                )
                .toString(),
        )
    }

    fun loadPinnedMessages(chatId: Long) {
        sendCommand(
            JSONObject()
                .put("type", "load_pinned_messages")
                .put("payload", JSONObject().put("chat_id", chatId))
                .toString(),
        )
    }

    fun loadMessagesBefore(chatId: Long, beforeMessageId: Long) {
        if (chatId == 0L || beforeMessageId <= 0L) return
        _state.update { current ->
            current.copy(
                loadingHistoryBefore = current.loadingHistoryBefore + (chatId to beforeMessageId),
                completedHistoryBefore = current.completedHistoryBefore - chatId,
                historyPageErrors = current.historyPageErrors - chatId,
            )
        }
        val accepted = sendCommand(
            JSONObject()
                .put("type", "load_messages_page")
                .put(
                    "payload",
                    JSONObject()
                        .put("chat_id", chatId)
                        .put("before_message_id", beforeMessageId)
                        .put("limit", 50),
                )
                .toString(),
        )
        if (!accepted) {
            _state.update { current ->
                current.copy(
                    loadingHistoryBefore = current.loadingHistoryBefore - chatId,
                    completedHistoryBefore = current.completedHistoryBefore + (chatId to beforeMessageId),
                    historyPageErrors = current.historyPageErrors + (chatId to "Could not load the pinned message history page"),
                )
            }
        }
    }

    fun toggleMessagePin(chatId: Long, messageId: Long, isPinned: Boolean) {
        if (chatId == 0L || messageId <= 0L) return
        val key = "$chatId:$messageId"
        _state.update {
            it.copy(
                pendingPinnedMessageIds = it.pendingPinnedMessageIds + key,
                pinnedError = null,
                pinnedFailureKey = null,
            )
        }
        val result = runCatching {
            lifecycle.dispatchCommand { currentHandle ->
                NativeBridge.sendCommand(
                    currentHandle,
                    JSONObject()
                        .put("type", if (isPinned) "unpin_message" else "pin_message")
                        .put("payload", JSONObject().put("chat_id", chatId).put("message_id", messageId))
                        .toString(),
                )
            }
        }.getOrElse { error ->
            _state.update {
                it.copy(
                    pendingPinnedMessageIds = it.pendingPinnedMessageIds - key,
                    pinnedError = error.message ?: error::class.java.simpleName,
                    pinnedFailureKey = key,
                )
            }
            return
        }
        if (result !in setOf(LifecycleDispatchResult.Accepted, LifecycleDispatchResult.Duplicate)) {
            _state.update {
                it.copy(
                    pendingPinnedMessageIds = it.pendingPinnedMessageIds - key,
                    pinnedError = "Rust rejected the pin command: $result",
                    pinnedFailureKey = key,
                )
            }
        }
        reportDispatchResult(result, "Rust rejected the pin command")
    }

    fun loadContacts(offset: Int = 0, limit: Int = 100) {
        if (offset < 0 || limit !in 1..100) {
            setErrorIfOpen("Invalid contacts page")
            return
        }
        _state.update { it.copy(contactsLoading = true, error = null) }
        val accepted = sendCommand(
            JSONObject()
                .put("type", "load_contacts")
                .put(
                    "payload",
                    JSONObject()
                        .put("offset", offset)
                        .put("limit", limit),
                )
                .toString(),
        )
        if (!accepted) _state.update { it.copy(contactsLoading = false) }
    }

    fun replyMessage(chatId: Long, replyToMessageId: Long, text: String) {
        if (chatId == 0L || replyToMessageId <= 0L || text.isBlank()) return
        sendCommand(
            JSONObject()
                .put("type", "reply_message")
                .put("payload", JSONObject()
                    .put("chat_id", chatId)
                    .put("reply_to_message_id", replyToMessageId)
                    .put("text", text))
                .toString(),
        )
    }

    fun sendText(chatId: Long, text: String): Boolean {
        if (chatId == 0L || text.isBlank() || _state.value.sendingText) return false
        val clientId = nextPositiveClientMessageId()
        _state.update {
            it.copy(
                activeTextSendId = clientId,
                lastTextSendId = clientId,
                activeTextSendChatId = chatId,
                activeTextSendText = text,
                failedTextSendId = null,
                failedTextSendText = null,
                textSendError = null,
            )
        }
        val accepted = sendCommand(reliableTextCommand(clientId, chatId, text))
        if (!accepted) {
            _state.update {
                if (it.activeTextSendId == clientId) it.copy(
                    activeTextSendId = null,
                    activeTextSendChatId = null,
                    activeTextSendText = null,
                ) else it
            }
        }
        return accepted
    }

    fun editMessage(chatId: Long, messageId: Long, text: String) {
        submitMessageMutation(
            chatId = chatId,
            messageId = messageId,
            commandType = "edit_message",
            extra = JSONObject().put("text", text),
        )
    }

    fun deleteMessageForEveryone(chatId: Long, messageId: Long) {
        submitMessageMutation(chatId, messageId, "delete_message_for_everyone")
    }

    fun deleteMessageForMe(chatId: Long, messageId: Long) {
        submitMessageMutation(chatId, messageId, "delete_message_for_me")
    }

    private fun submitMessageMutation(
        chatId: Long,
        messageId: Long,
        commandType: String,
        extra: JSONObject = JSONObject(),
    ) {
        if (chatId == 0L || messageId <= 0L || _state.value.messageMutationPending) return
        _state.update {
            it.copy(
                messageMutationPending = true,
                messageMutationError = null,
                messageMutationChatId = chatId,
                messageMutationMessageId = messageId,
            )
        }
        val payload = JSONObject().put("chat_id", chatId).put("message_id", messageId)
        extra.keys().forEach { key -> payload.put(key, extra.get(key)) }
        val accepted = sendCommand(
            JSONObject().put("type", commandType).put("payload", payload).toString(),
        )
        if (!accepted) {
            _state.update {
                it.copy(
                    messageMutationPending = false,
                    messageMutationError = "Не удалось отправить запрос в Telegram",
                )
            }
        }
    }

    fun sendLocation(
        chatId: Long,
        latitude: Double,
        longitude: Double,
        accuracyRadiusMeters: Int?,
    ) {
        if (!latitude.isFinite() || !longitude.isFinite() ||
            latitude !in -90.0..90.0 || longitude !in -180.0..180.0
        ) {
            setErrorIfOpen("Invalid location coordinates")
            return
        }
        if (accuracyRadiusMeters != null && accuracyRadiusMeters !in 0..1_500) {
            setErrorIfOpen("Invalid location accuracy radius")
            return
        }
        val point = JSONObject().put("latitude", latitude).put("longitude", longitude)
        accuracyRadiusMeters?.let { point.put("accuracy_radius_meters", it) }
        sendCommand(
            JSONObject()
                .put("type", "send_location")
                .put("payload", JSONObject().put("chat_id", chatId).put("location", point))
                .toString(),
        )
    }

    fun reportLocationError(message: String) {
        setErrorIfOpen(message)
    }

    fun forwardMessages(sourceChatId: Long, messageIds: List<Long>, destinationChatId: Long) {
        if (sourceChatId == 0L || destinationChatId == 0L || messageIds.isEmpty()) return
        val ids = JSONArray().apply { messageIds.forEach { id -> put(id) } }
        _state.update { it.copy(busy = true, error = null, forwardError = null) }
        val accepted = sendCommand(
            JSONObject()
                .put("type", "forward_messages")
                .put("payload", JSONObject()
                    .put("request_id", java.util.UUID.randomUUID().toString())
                    .put("source_chat_id", sourceChatId)
                    .put("message_ids", ids)
                    .put("destination_chat_id", destinationChatId))
                .toString(),
        )
        if (!accepted) _state.update { it.copy(busy = false, error = "Не удалось отправить запрос пересылки") }
    }

    fun setMessageReaction(chatId: String, messageId: String, reaction: ConversationReactionUi?) {
        val chat = chatId.toLongOrNull() ?: return
        val message = messageId.toLongOrNull() ?: return
        if (chat == 0L || message <= 0L) return
        val reactionValue = if (reaction?.type == "custom_emoji") reaction.value.toLongOrNull() else null
        if (reaction?.type == "custom_emoji" && (reactionValue == null || reactionValue <= 0L)) return
        val key = "$chat:$message"
        if (key in _state.value.pendingReactionKeys) return
        _state.update {
            it.copy(
                pendingReactionKeys = it.pendingReactionKeys + key,
                reactionErrors = it.reactionErrors - key,
            )
        }

        val command = if (reaction == null) {
            JSONObject()
                .put("type", "remove_reaction")
                .put("payload", JSONObject().put("chat_id", chat).put("message_id", message))
        } else {
            val reactionType = if (reaction.type == "custom_emoji") "custom_emoji" else "emoji"
            val value: Any = reactionValue ?: reaction.value
            JSONObject()
                .put("type", "add_reaction")
                .put(
                    "payload",
                    JSONObject()
                        .put("chat_id", chat)
                        .put("message_id", message)
                        .put("reaction", JSONObject().put("type", reactionType).put("value", value)),
                )
        }
        if (!sendCommand(command.toString())) {
            _state.update {
                it.copy(
                    pendingReactionKeys = it.pendingReactionKeys - key,
                    reactionErrors = it.reactionErrors + (key to "Не удалось отправить реакцию"),
                )
            }
        }
    }

    fun deleteMessages(chatId: Long, messageIds: List<Long>) {
        if (chatId == 0L || messageIds.isEmpty() || messageIds.size > 100 ||
            messageIds.any { it <= 0L } || messageIds.distinct().size != messageIds.size
        ) return
        _state.update { it.copy(deletingMessages = true, deleteError = null, busy = true, error = null) }
        val ids = JSONArray().apply { messageIds.forEach { put(it) } }
        val accepted = sendCommand(
            JSONObject()
                .put("type", "delete_messages")
                .put("payload", JSONObject().put("chat_id", chatId).put("message_ids", ids))
                .toString(),
        )
        if (!accepted) {
            _state.update {
                it.copy(
                    deletingMessages = false,
                    deleteError = "Не удалось отправить запрос удаления",
                    busy = false,
                )
            }
        }
    }

    fun votePoll(poll: ConversationPollUi, optionIds: List<List<Int>>) {
        if (poll.closed) {
            _state.update {
                it.copy(pollVoteErrors = it.pollVoteErrors + (poll.id to "Опрос закрыт"))
            }
            return
        }

        val optionsJson = JSONArray()
        optionIds.forEach { optionId ->
            val optionJson = JSONArray()
            optionId.forEach { byte -> optionJson.put(byte) }
            optionsJson.put(optionJson)
        }

        _state.update {
            it.copy(
                votingPollIds = it.votingPollIds + poll.id,
                pollVoteErrors = it.pollVoteErrors - poll.id,
            )
        }

        val accepted = sendCommand(
            JSONObject()
                .put("type", "vote_poll")
                .put(
                    "payload",
                    JSONObject()
                        .put("poll_id", poll.id)
                        .put("chat_id", poll.chatId)
                        .put("message_id", poll.messageId)
                        .put("option_ids", optionsJson),
                )
                .toString(),
        )
        if (!accepted) {
            _state.update {
                it.copy(
                    votingPollIds = it.votingPollIds - poll.id,
                    pollVoteErrors = it.pollVoteErrors +
                        (poll.id to "Не удалось отправить голос"),
                )
            }
        }
    }

    fun downloadMedia(
        chatId: Long,
        messageId: Long,
        attachmentIndex: Int,
        retry: Boolean = false,
    ) {
        sendCommand(
            JSONObject()
                .put("type", if (retry) "retry_media_download" else "download_media")
                .put(
                    "payload",
                    JSONObject()
                        .put("chat_id", chatId)
                        .put("message_id", messageId)
                        .put("attachment_index", attachmentIndex),
                )
                .toString(),
        )
    }

    fun cancelMediaDownload(
        chatId: Long,
        messageId: Long,
        attachmentIndex: Int,
    ) {
        sendCommand(
            JSONObject()
                .put("type", "cancel_media_download")
                .put(
                    "payload",
                    JSONObject()
                        .put("chat_id", chatId)
                        .put("message_id", messageId)
                        .put("attachment_index", attachmentIndex),
                )
                .toString(),
        )
    }

    private fun markRead(chatId: Long, maxMessageId: Long) {
        if (chatId == 0L || maxMessageId <= 0L) return
        sendCommand(
            JSONObject()
                .put("type", "mark_read")
                .put(
                    "payload",
                    JSONObject()
                        .put("chat_id", chatId)
                        .put("max_message_id", maxMessageId),
                )
                .toString(),
        )
    }

    private fun sendCommand(commandJson: String): Boolean {
        if (!NativeLibrary.isLoaded) {
            setErrorIfOpen("Native bridge is not available")
            return false
        }

        val result = runCatching {
            lifecycle.dispatchCommand { currentHandle ->
                NativeBridge.sendCommand(currentHandle, commandJson)
            }
        }.getOrElse { error ->
            setErrorIfOpen(error.message ?: error::class.java.simpleName)
            return false
        }

        reportDispatchResult(result, "Rust rejected the command")
        return result == LifecycleDispatchResult.Accepted ||
            result == LifecycleDispatchResult.Duplicate
    }

    private fun reportDispatchResult(
        result: LifecycleDispatchResult,
        rejectedMessage: String,
    ) {
        when (result) {
            LifecycleDispatchResult.Accepted -> Unit
            LifecycleDispatchResult.Duplicate -> Unit
            LifecycleDispatchResult.Closed -> Unit
            LifecycleDispatchResult.NoHandle -> setErrorIfOpen("Native bridge is not available")
            LifecycleDispatchResult.Rejected -> setErrorIfOpen(rejectedMessage)
        }
    }

    private fun setErrorIfOpen(message: String) {
        lifecycle.runIfOpen {
            _state.update {
                it.copy(
                    busy = false,
                    contactsLoading = false,
                    error = message,
                )
            }
        }
    }

    private fun markMessageMutationUnconfirmed(message: String) {
        _state.update {
            val mutation = messageMutationStateAfterSessionChange(
                pending = it.messageMutationPending,
                currentError = it.messageMutationError,
                reason = message,
            )
            it.copy(
                messageMutationPending = mutation.pending,
                messageMutationError = mutation.error,
            )
        }
    }

    private fun onNativeEvent(generation: Long, type: String, payload: String) {
        if (!lifecycle.onNativeEvent(generation, type)) return
        lifecycle.runIfCurrent(generation) {
            handleNativeEvent(type, payload)
        }
    }

    private fun handleNativeEvent(type: String, payload: String) {
        when (type) {
            "connection_lifecycle" -> {
                _state.update { current ->
                    current.applyConnectionLifecycleAndGeneration(payload).copy(
                        lastEventType = type,
                        lastEventPayload = payload,
                    )
                }
            }
            "messages_deleted" -> {
                val deleted = parseDeletedMessages(payload)
                _state.update { current ->
                    if (deleted == null) {
                        current.copy(lastEventType = type, lastEventPayload = payload)
                    } else {
                        current.finishBulkDelete(deleted.first, deleted.second).copy(
                            lastEventType = type,
                            lastEventPayload = payload,
                        )
                    }
                }
            }
            "state" -> applyStatePayload(payload)
            "message_send_state_changed" -> {
                val send = parseOutgoingTextSendEvent(payload)
                _state.update { current ->
                    if (send == null) current.copy(lastEventType = type, lastEventPayload = "")
                    else current.applyOutgoingTextSend(send).copy(
                        lastEventType = type,
                        lastEventPayload = "",
                    )
                }
            }
            "authorization_changed" -> applyAuthorizationEvent(payload)
            "chats_loaded" -> {
                _state.update {
                    it.copy(
                        busy = false,
                        lastEventType = type,
                        lastEventPayload = payload,
                        error = null,
                    )
                }
            }
            "profile_updated" -> {
                val profile = runCatching {
                    val root = JSONObject(payload).optJSONObject("payload") ?: JSONObject()
                    parsePeerProfile(root)
                }.getOrNull()
                _state.update { current ->
                    if (profile == null) current.copy(
                        profileLoadingPeerId = null,
                        profileError = "Could not read peer profile",
                        lastEventType = type,
                        lastEventPayload = "",
                    ) else current.copy(
                        profiles = boundedProfiles(current.profiles, profile),
                        profileLoadingPeerId = null,
                        profileError = null,
                        lastEventType = type,
                        lastEventPayload = "{\"peer_id\":${profile.peerId}}",
                    )
                }
            }
            "contacts_loaded" -> {
                val page = parseContactPage(payload)
                if (page == null) {
                    _state.update {
                        it.copy(contactsLoading = false, error = "Invalid contacts response")
                    }
                } else {
                    _state.update {
                        val contacts = if (page.offset == 0) {
                            page.contacts
                        } else {
                            (it.contacts + page.contacts).distinctBy(ContactListEntry::userId)
                                .sortedBy(ContactListEntry::userId)
                        }
                        it.copy(
                            contacts = contacts,
                            contactsLoading = false,
                            contactTotal = page.total,
                            lastEventType = type,
                            // Keep contact names and usernames out of diagnostic state.
                            lastEventPayload = "{\"offset\":${page.offset},\"count\":${page.contacts.size},\"total\":${page.total}}",
                            error = null,
                        )
                    }
                }
            }
            "contacts_refresh_requested" -> loadContacts(offset = 0)
            "messages_loaded" -> {
                val target = parseReadTarget(payload)
                _state.update {
                    it.copy(
                        busy = false,
                        contactsLoading = false,
                        lastEventType = type,
                        lastEventPayload = payload,
                        error = null,
                    )
                }
                target?.let { (chatId, maxMessageId) ->
                    markRead(chatId, maxMessageId)
                }
            }
            "poll_updated" -> {
                val pollId = parsePollEventId(payload)
                _state.update {
                    it.copy(
                        votingPollIds = if (pollId != null) {
                            it.votingPollIds - pollId
                        } else {
                            it.votingPollIds
                        },
                        pollVoteErrors = if (pollId != null) {
                            it.pollVoteErrors - pollId
                        } else {
                            it.pollVoteErrors
                        },
                        lastEventType = type,
                        lastEventPayload = payload,
                    )
                }
            }
            "poll_vote_submitted" -> {
                _state.update {
                    it.copy(
                        lastEventType = type,
                        lastEventPayload = payload,
                    )
                }
            }
            "poll_vote_failed" -> {
                val failure = parsePollVoteFailure(payload)
                _state.update {
                    if (failure == null) {
                        it.copy(lastEventType = type, lastEventPayload = payload)
                    } else {
                        it.copy(
                            votingPollIds = it.votingPollIds - failure.first,
                            pollVoteErrors = it.pollVoteErrors + failure,
                            lastEventType = type,
                            lastEventPayload = payload,
                        )
                    }
                }
            }
            "messages_forwarded" -> {
                val (succeeded, failed, firstError) = runCatching {
                    val result = JSONObject(payload).optJSONObject("payload") ?: JSONObject()
                    val outcomes = result.optJSONArray("outcomes") ?: JSONArray()
                    var ok = 0
                    var errors = 0
                    var detail: String? = null
                    for (index in 0 until outcomes.length()) {
                        val outcome = outcomes.optJSONObject(index) ?: continue
                        if (outcome.optString("status") == "forwarded") ok++ else {
                            errors++
                            if (detail == null) detail = outcome.optString("error").takeIf(String::isNotBlank)
                        }
                    }
                    Triple(ok, errors, detail)
                }.getOrDefault(Triple(0, 1, null))
                _state.update {
                    it.copy(
                        busy = false,
                        lastEventType = type,
                        lastEventPayload = payload,
                        forwardError = if (failed == 0 && succeeded > 0) null else "Переслано $succeeded, не удалось: $failed${firstError?.let { detail -> " — $detail" }.orEmpty()}",
                        forwardSuccessCount = if (failed == 0 && succeeded > 0) it.forwardSuccessCount + 1 else it.forwardSuccessCount,
                    )
                }
            }
            "connection_session_started" -> {
                markMessageMutationUnconfirmed("Сессия Telegram обновилась; результат действия не подтверждён")
                _state.update { it.copy(lastEventType = type, lastEventPayload = payload) }
            }
            "message_mutation_failed" -> {
                val failure = runCatching {
                    val payloadObject = JSONObject(payload).optJSONObject("payload") ?: JSONObject()
                    val chatId = payloadObject.optLong("chat_id")
                    val messageId = payloadObject.optLong("message_id")
                    val error = payloadObject.optString("error").ifBlank { "Telegram отклонил действие" }
                    Triple(chatId, messageId, error)
                }.getOrNull()
                _state.update {
                    if (failure != null && it.messageMutationPending && matchesMutationTarget(
                            it.messageMutationChatId,
                            it.messageMutationMessageId,
                            failure.first,
                            failure.second,
                        )
                    ) {
                        it.copy(
                            messageMutationPending = false,
                            messageMutationError = failure.third,
                            lastEventType = type,
                            lastEventPayload = payload,
                        )
                    } else it.copy(lastEventType = type, lastEventPayload = payload)
                }
            }
            "message_mutation_succeeded" -> {
                val target = runCatching {
                    val body = JSONObject(payload).optJSONObject("payload") ?: JSONObject()
                    body.optLong("chat_id") to body.optLong("message_id")
                }.getOrDefault(0L to 0L)
                _state.update {
                    if (it.messageMutationPending && mutationEventAcknowledges(
                            type,
                            it.messageMutationChatId,
                            it.messageMutationMessageId,
                            target.first,
                            target.second,
                        )
                    ) {
                        it.copy(
                            messageMutationPending = false,
                            messageMutationError = null,
                            messageMutationSuccessCount = it.messageMutationSuccessCount + 1,
                            messageMutationSuccessChatId = target.first,
                            messageMutationSuccessMessageId = target.second,
                            lastEventType = type,
                            lastEventPayload = payload,
                        )
                    } else it.copy(lastEventType = type, lastEventPayload = payload)
                }
            }
            "message_edited" -> _state.update {
                it.copy(lastEventType = type, lastEventPayload = payload)
            }
            "message_reactions_updated" -> {
                val update = parseReactionUpdate(payload)
                _state.update { current ->
                    if (update == null) current.copy(lastEventType = type, lastEventPayload = payload)
                    else {
                        val (chatId, messageId, reactions, revision) = update
                        current.applyReactionUpdate(chatId, messageId, reactions, revision).copy(
                            lastEventType = type,
                            lastEventPayload = payload,
                        )
                    }
                }
            }
            "reaction_operation_succeeded" -> completeReactionOperation(type, payload)
            "reaction_operation_failed" -> {
                val failure = parseReactionFailure(payload)
                _state.update {
                    if (failure == null) it.copy(lastEventType = type, lastEventPayload = payload)
                    else it.copy(
                        pendingReactionKeys = it.pendingReactionKeys - failure.first,
                        reactionErrors = it.reactionErrors + failure,
                        lastEventType = type,
                        lastEventPayload = payload,
                    )
                }
            }
            "message_added",
            "chat_read",
            -> {
                _state.update {
                    it.copy(
                        lastEventType = type,
                        lastEventPayload = payload,
                    )
                }
            }
            "messages_page_loaded", "messages_page_load_failed" -> {
                val body = runCatching { JSONObject(payload).optJSONObject("payload") }.getOrNull()
                val chatId = body?.optLong("chat_id") ?: 0L
                val beforeId = body?.takeIf { !it.isNull("before_message_id") }?.optLong("before_message_id")
                val error = if (type == "messages_page_load_failed") body?.optString("error")?.takeIf { !it.isNullOrBlank() } else null
                _state.update { current ->
                    val matchesPending = beforeId != null && current.loadingHistoryBefore[chatId] == beforeId
                    current.copy(
                        loadingHistoryBefore = if (matchesPending) current.loadingHistoryBefore - chatId else current.loadingHistoryBefore,
                        completedHistoryBefore = if (matchesPending) current.completedHistoryBefore + (chatId to beforeId!!) else current.completedHistoryBefore,
                        historyPageErrors = if (matchesPending && error != null) current.historyPageErrors + (chatId to error) else if (matchesPending) current.historyPageErrors - chatId else current.historyPageErrors,
                        lastEventType = type,
                        lastEventPayload = payload,
                    )
                }
            }
            "pinned_messages_loaded" -> {
                _state.update {
                    it.copy(
                        lastEventType = type,
                        lastEventPayload = payload,
                    )
                }
            }
            "pinned_messages_changed" -> {
                val changed = runCatching {
                    val event = JSONObject(payload).optJSONObject("payload") ?: return@runCatching null
                    val chatId = event.optLong("chat_id")
                    val ids = event.optJSONArray("message_ids") ?: JSONArray()
                    val pts = event.takeIf { !it.isNull("pts") }?.optLong("pts")?.toInt()
                    Triple(chatId, (0 until ids.length()).map { ids.optLong(it) }, pts)
                }.getOrNull()
                _state.update { current ->
                    current.copy(
                        lastEventType = type,
                        lastEventPayload = payload,
                        pendingPinnedMessageIds = changed?.takeIf { (chatId, ids, pts) ->
                            shouldResolvePinnedAction(pts, chatId, ids)
                        }?.let { (chatId, ids, _) ->
                            current.pendingPinnedMessageIds - ids.map { "$chatId:$it" }.toSet()
                        } ?: current.pendingPinnedMessageIds,
                        pinnedError = if (changed?.let { (chatId, ids, pts) -> shouldClearPinnedFailure(current.pinnedFailureKey, chatId, ids, pts) } == true) null else current.pinnedError,
                        pinnedFailureKey = if (changed?.let { (chatId, ids, pts) -> shouldClearPinnedFailure(current.pinnedFailureKey, chatId, ids, pts) } == true) null else current.pinnedFailureKey,
                    )
                }
            }
            "pinned_message_failed" -> {
                val event = runCatching { JSONObject(payload).optJSONObject("payload") }.getOrNull()
                val chatId = event?.optLong("chat_id") ?: 0L
                val messageId = event?.optLong("message_id") ?: 0L
                val error = event?.optString("error")?.takeIf { !it.isNullOrBlank() } ?: "Pin action failed"
                val key = "$chatId:$messageId"
                _state.update {
                    it.copy(
                        pendingPinnedMessageIds = it.pendingPinnedMessageIds - key,
                        pinnedError = error,
                        pinnedFailureKey = key,
                        lastEventType = type,
                        lastEventPayload = payload,
                    )
                }
            }
            "error" -> {
                val message = runCatching {
                    val root = JSONObject(payload)
                    root.optString("payload").ifBlank {
                        root.optString("message", payload)
                    }
                }.getOrDefault(payload)

                _state.update {
                    it.copy(
                        busy = false,
                        profileLoadingPeerId = if (it.profileLoadingPeerId != null) null else it.profileLoadingPeerId,
                        profileError = if (it.profileLoadingPeerId != null) message else it.profileError,
                        contactsLoading = false,
                        deletingMessages = false,
                        deleteError = if (it.deletingMessages) message else it.deleteError,
                        lastEventType = type,
                        lastEventPayload = payload,
                        error = message,
                    )
                }
            }
            else -> {
                _state.update {
                    it.copy(
                        lastEventType = type,
                        lastEventPayload = payload,
                    )
                }
            }
        }
    }

    private fun applyAuthorizationEvent(payload: String) {
        val authorization = runCatching {
            val root = JSONObject(payload)
            parseAuthorization(root.optString("payload"))
        }.getOrDefault(AuthorizationUiStep.Closed)

        val loggedOut = authorization == AuthorizationUiStep.Closed
        _state.update { current ->
            current.clearPendingReactionsAfterConnectionChange().copy(
                chats = if (loggedOut) emptyList() else current.chats,
                profiles = if (loggedOut) emptyMap() else current.profiles,
                profileLoadingPeerId = if (loggedOut) null else current.profileLoadingPeerId,
                profileError = if (loggedOut) null else current.profileError,
                contacts = if (loggedOut) emptyList() else current.contacts,
                contactTotal = if (loggedOut) 0 else current.contactTotal,
                contactsLoading = false,
                chatCapabilities = if (loggedOut) emptyMap() else current.chatCapabilities,
                messages = if (loggedOut) emptyMap() else current.messages,
                pendingReactionKeys = if (loggedOut) emptySet() else current.pendingReactionKeys,
                reactionConnectionToken = if (loggedOut) null else current.reactionConnectionToken,
                reactionErrors = if (loggedOut) emptyMap() else current.reactionErrors,
                votingPollIds = if (loggedOut) emptySet() else current.votingPollIds,
                pollVoteErrors = if (loggedOut) emptyMap() else current.pollVoteErrors,
                pinnedMessages = if (loggedOut) emptyMap() else current.pinnedMessages,
                pendingPinnedMessageIds = if (loggedOut) emptySet() else current.pendingPinnedMessageIds,
                pinnedError = if (loggedOut) null else current.pinnedError,
                pinnedFailureKey = if (loggedOut) null else current.pinnedFailureKey,
                loadingHistoryBefore = if (loggedOut) emptyMap() else current.loadingHistoryBefore,
                completedHistoryBefore = if (loggedOut) emptyMap() else current.completedHistoryBefore,
                historyPageErrors = if (loggedOut) emptyMap() else current.historyPageErrors,
                forwardError = if (loggedOut) null else current.forwardError,
                forwardSuccessCount = if (loggedOut) 0 else current.forwardSuccessCount,
                messageMutationPending = if (loggedOut) false else current.messageMutationPending,
                messageMutationError = if (loggedOut) null else current.messageMutationError,
                outbox = if (authorization == AuthorizationUiStep.Ready) current.outbox else emptyMap(),
                activeTextSendId = if (authorization == AuthorizationUiStep.Ready) current.activeTextSendId else null,
                lastTextSendId = if (authorization == AuthorizationUiStep.Ready) current.lastTextSendId else null,
                activeTextSendChatId = if (authorization == AuthorizationUiStep.Ready) current.activeTextSendChatId else null,
                activeTextSendText = if (authorization == AuthorizationUiStep.Ready) current.activeTextSendText else null,
                textSendError = if (authorization == AuthorizationUiStep.Ready) current.textSendError else null,
                authorization = authorization,
                selectionGeneration = if (
                    authorization != AuthorizationUiStep.Ready &&
                    current.authorization == AuthorizationUiStep.Ready
                ) {
                    current.selectionGeneration + 1
                } else {
                    current.selectionGeneration
                },
                deletingMessages = false,
                deleteError = null,
                pendingPinnedMessageIds = if (authorization != AuthorizationUiStep.Ready) emptySet() else current.pendingPinnedMessageIds,
                pinnedMessages = if (authorization != AuthorizationUiStep.Ready) emptyMap() else current.pinnedMessages,
                pinnedError = if (authorization != AuthorizationUiStep.Ready) null else current.pinnedError,
                pinnedFailureKey = if (authorization != AuthorizationUiStep.Ready) null else current.pinnedFailureKey,
                busy = false,
                contactsLoading = false,
                lastEventType = "authorization_changed",
                lastEventPayload = payload,
                error = null,
            )
        }

        if (authorization == AuthorizationUiStep.Ready) {
            loadChats()
            loadContacts(offset = 0)
        }
    }

    private fun applyStatePayload(payload: String) {
        runCatching {
            val root = JSONObject(payload)
            val account = root.getJSONObject("account")
            val authorization = parseAuthorization(account.optString("authorization"))
            val chats = parseChats(account.optJSONArray("chats") ?: JSONArray())
            val polls = parsePolls(root.optJSONObject("polls"))
            val readOutboxMaxIds = parseReadOutboxMaxIds(root.optJSONObject("read_receipts"))
            val messages = parseMessages(root.optJSONObject("messages"), polls, readOutboxMaxIds)
            val profiles = parseProfiles(root.optJSONObject("profiles") ?: JSONObject())
            val contacts = parseContactListState(root.optJSONObject("contacts"))
            val capabilities = parseChatCapabilities(root.optJSONObject("chat_capabilities"))
            val pinnedMessages = parsePinnedMessages(root.optJSONObject("pinned_messages"))
            val outbox = parseOutgoingTextOutbox(root.optJSONObject("outbox"))
            val ledger = parseOutgoingTextOutbox(root.optJSONObject("send_ledger"))
            ParsedNativeState(authorization, chats, messages, pinnedMessages, contacts, profiles, capabilities, outbox + ledger)
        }.onSuccess { parsed ->
            _state.update {
                it.copy(
                    authorization = parsed.authorization,
                    chats = parsed.chats.map { chat ->
                        chat.copy(
                            capabilities = parsed.chatCapabilities[chat.id]
                                ?: ChatCapabilitiesUi(),
                        )
                    },
                    contacts = parsed.contacts,
                    contactTotal = maxOf(it.contactTotal, parsed.contacts.size),
                    messages = parsed.messages,
                    pinnedMessages = parsed.pinnedMessages,
                    profiles = parsed.profiles,
                    chatCapabilities = parsed.chatCapabilities,
                    outbox = parsed.outbox,
                    busy = false,
                    contactsLoading = false,
                    lastEventType = "state",
                    lastEventPayload = payloadWithoutPrivateState(payload),
                    error = null,
                ).let { parsedState ->
                    parsedState.activeTextSendId?.let(parsed.outbox::get)?.let(parsedState::applyOutgoingTextSend)
                        ?: parsedState
                }
            }
        }.onFailure { error ->
            _state.update {
                it.copy(
                    busy = false,
                    contactsLoading = false,
                    error = "Invalid Rust state payload: ${error.message}",
                )
            }
        }
    }

    private fun parseProfiles(root: JSONObject): Map<Long, PeerProfileUi> = buildMap {
        val keys = root.keys()
        while (keys.hasNext()) {
            val key = keys.next()
            val profile = root.optJSONObject(key) ?: continue
            val parsed = runCatching { parsePeerProfile(profile) }.getOrNull() ?: continue
            put(parsed.peerId, parsed)
        }
    }

    // Profiles are fetched on demand, so retain only the most recent cache entries.
    private fun boundedProfiles(
        current: Map<Long, PeerProfileUi>,
        profile: PeerProfileUi,
    ): Map<Long, PeerProfileUi> = (current + (profile.peerId to profile))
        .entries
        .toList()
        .takeLast(128)
        .associate { it.toPair() }

    private fun parsePeerProfile(root: JSONObject): PeerProfileUi {
        val presenceObject = root.optJSONObject("presence")
        val presence = presenceObject?.optString("status")?.takeIf { it.isNotBlank() && it != "unknown" }
            ?.let { status ->
                if (status == "offline") {
                    presenceObject.optLong("last_seen_unix").takeIf { it > 0L }?.let { "Offline since $it" } ?: "Offline"
                } else status.replaceFirstChar(Char::uppercase)
            }
        val avatar = root.optJSONObject("avatar")
        return PeerProfileUi(
            peerId = root.getLong("peer_id"),
            kind = root.optString("kind", "unknown"),
            displayName = root.optString("display_name").takeIf(String::isNotBlank),
            firstName = root.optString("first_name").takeIf(String::isNotBlank),
            lastName = root.optString("last_name").takeIf(String::isNotBlank),
            username = root.optString("username").takeIf(String::isNotBlank),
            bio = root.optString("bio").takeIf(String::isNotBlank),
            memberCount = root.optInt("member_count").takeIf { root.has("member_count") && !root.isNull("member_count") },
            presence = presence,
            isBot = root.optionalBoolean("is_bot"),
            isVerified = root.optionalBoolean("is_verified"),
            isPremium = root.optionalBoolean("is_premium"),
            canSendMessages = root.optionalBoolean("can_send_messages"),
            canInviteUsers = root.optionalBoolean("can_invite_users"),
            canChangeInfo = root.optionalBoolean("can_change_info"),
            canPostMessages = root.optionalBoolean("can_post_messages"),
            avatarFileId = avatar?.optString("file_id")?.takeIf(String::isNotBlank),
            avatarLocalPath = avatar?.optString("local_path")?.takeIf(String::isNotBlank),
        )
    }

    private fun JSONObject.optionalBoolean(name: String): Boolean? =
        if (has(name) && !isNull(name)) optBoolean(name) else null

    private fun payloadWithoutPrivateState(payload: String): String = runCatching {
        JSONObject(payload).apply {
            remove("contacts")
            remove("profiles")
        }.toString()
    }.getOrDefault("")

    private fun parseChats(array: JSONArray): List<ChatListEntry> = buildList {
        for (index in 0 until array.length()) {
            val chat = array.optJSONObject(index) ?: continue
            val lastMessage = chat.optJSONObject("last_message")
            add(
                ChatListEntry(
                    id = chat.optLong("id"),
                    title = chat.optString("title", "Chat"),
                    unreadCount = chat.optLong("unread_count", 0L),
                    lastMessage = lastMessage?.let {
                        ChatListPreview(
                            text = it.optString("text"),
                            timestampUnix = it.optLong("timestamp_unix"),
                            outgoing = it.optBoolean("outgoing"),
                        )
                    },
                ),
            )
        }
    }

    private fun parsePolls(root: JSONObject?): Map<Pair<Long, Long>, ConversationPollUi> {
        if (root == null) return emptyMap()

        return buildMap {
            val keys = root.keys()
            while (keys.hasNext()) {
                val key = keys.next()
                val poll = root.optJSONObject(key) ?: continue
                val pollId = poll.optLong("id")
                val chatId = poll.optLong("chat_id")
                val messageId = poll.optLong("message_id")
                if (pollId == 0L || chatId == 0L || messageId <= 0L) continue

                val optionsJson = poll.optJSONArray("options") ?: JSONArray()
                val options = buildList {
                    for (index in 0 until optionsJson.length()) {
                        val option = optionsJson.optJSONObject(index) ?: continue
                        val optionIdJson = option.optJSONArray("option_id") ?: JSONArray()
                        val optionId = buildList {
                            for (byteIndex in 0 until optionIdJson.length()) {
                                add(optionIdJson.optInt(byteIndex).coerceIn(0, 255))
                            }
                        }
                        val voterCount = if (option.has("voter_count") && !option.isNull("voter_count")) {
                            option.optInt("voter_count")
                        } else {
                            null
                        }
                        val correct = if (option.has("correct") && !option.isNull("correct")) {
                            option.optBoolean("correct")
                        } else {
                            null
                        }
                        add(
                            ConversationPollOptionUi(
                                optionId = optionId,
                                text = option.optString("text"),
                                voterCount = voterCount,
                                chosen = option.optBoolean("chosen"),
                                correct = correct,
                            ),
                        )
                    }
                }

                val totalVoters = if (poll.has("total_voters") && !poll.isNull("total_voters")) {
                    poll.optInt("total_voters")
                } else {
                    null
                }
                val explanation = if (poll.has("explanation") && !poll.isNull("explanation")) {
                    poll.optString("explanation").takeIf(String::isNotBlank)
                } else {
                    null
                }

                put(
                    chatId to messageId,
                    ConversationPollUi(
                        id = pollId,
                        chatId = chatId,
                        messageId = messageId,
                        question = poll.optString("question"),
                        options = options,
                        totalVoters = totalVoters,
                        publicVoters = poll.optBoolean("public_voters"),
                        multipleChoice = poll.optBoolean("multiple_choice"),
                        quiz = poll.optBoolean("quiz"),
                        closed = poll.optBoolean("closed"),
                        explanation = explanation,
                    ),
                )
            }
        }
    }

    private fun parseMessages(
        root: JSONObject?,
        polls: Map<Pair<Long, Long>, ConversationPollUi>,
        readOutboxMaxIds: Map<Long, Long>,
    ): Map<Long, List<ConversationMessageUi>> {
        if (root == null) return emptyMap()

        return buildMap {
            val keys = root.keys()
            while (keys.hasNext()) {
                val chatKey = keys.next()
                val chatId = chatKey.toLongOrNull() ?: continue
                val array = root.optJSONArray(chatKey) ?: continue
                val messages = buildList {
                    for (index in 0 until array.length()) {
                        val message = array.optJSONObject(index) ?: continue
                        val timestamp = message.optLong("timestamp_unix")
                        val forwarded = message.optJSONObject("forwarded_from")
                        val forwardLabel = forwarded?.optString("sender_display_name")?.takeIf(String::isNotBlank)
                            ?: forwarded?.optLong("sender_id")?.takeIf { it != 0L }?.let { "пользователь $it" }
                            ?: forwarded?.optLong("channel_id")?.takeIf { it != 0L }?.let { "канал $it" }
                            ?: forwarded?.optLong("chat_id")?.takeIf { it != 0L }?.let { "чат $it" }
                        add(
                            ConversationMessageUi(
                                id = message.optLong("id").toString(),
                                chatId = chatId.toString(),
                                text = message.optString("text"),
                                timestamp = formatTimestamp(timestamp),
                                isOutgoing = message.optBoolean("outgoing"),
                                isRead = message.optBoolean("outgoing") &&
                                    message.optLong("id") <= (readOutboxMaxIds[chatId] ?: 0L),
                                attachments = parseMediaAttachments(
                                    message.optJSONArray("attachments") ?: JSONArray(),
                                ),
                                forwardedFrom = forwardLabel,
                                senderId = message.optLong("sender_id"),
                                replyToMessageId = message.optJSONObject("reply_to")
                                    ?.optLong("message_id")
                                    ?.takeIf { it > 0L }
                                    ?.toString(),
                                serviceAction = parseServiceAction(message),
                                location = parseLocation(message.optJSONObject("location")),
                                poll = polls[chatId to message.optLong("id")],
                                reactions = parseReactions(message.optJSONArray("reactions")),
                                reactionRevision = parseReactionRevision(message.optJSONObject("reaction_revision")),
                            ),
                        )
                    }
                }
                put(chatId, messages)
            }
        }
    }

    private fun parseReadOutboxMaxIds(root: JSONObject?): Map<Long, Long> {
        if (root == null) return emptyMap()
        return buildMap {
            val keys = root.keys()
            while (keys.hasNext()) {
                val chatId = keys.next().toLongOrNull() ?: continue
                val receipt = root.optJSONObject(chatId.toString()) ?: continue
                put(chatId, receipt.optLong("outbox_max_message_id", 0L).coerceAtLeast(0L))
            }
        }
    }

    private fun parseMediaAttachments(array: JSONArray): List<MediaAttachmentUi> = buildList {
        for (index in 0 until array.length()) {
            val attachment = array.optJSONObject(index) ?: continue
            val reference = attachment.optString("file_reference").takeIf { it.isNotBlank() }
                ?: continue
            val download = attachment.optJSONObject("download_state")
            val status = download?.optString("status").orEmpty()
            val totalBytes = download
                ?.takeIf { it.has("total_bytes") && !it.isNull("total_bytes") }
                ?.optLong("total_bytes")
                ?.takeIf { it > 0L }
            val localPath = download
                ?.optString("local_path")
                ?.takeIf { it.isNotBlank() }
            val reason = download
                ?.optString("reason")
                ?.takeIf { it.isNotBlank() }

            add(
                MediaAttachmentUi(
                    index = index,
                    kind = mediaKindFromWire(attachment.optString("kind")),
                    fileReference = reference,
                    sizeBytes = attachment
                        .takeIf { it.has("size_bytes") && !it.isNull("size_bytes") }
                        ?.optLong("size_bytes")
                        ?.takeIf { it >= 0L },
                    mimeType = attachment
                        .optString("mime_type")
                        .takeIf { it.isNotBlank() },
                    width = attachment
                        .takeIf { it.has("width") && !it.isNull("width") }
                        ?.optInt("width")
                        ?.takeIf { it > 0 },
                    height = attachment
                        .takeIf { it.has("height") && !it.isNull("height") }
                        ?.optInt("height")
                        ?.takeIf { it > 0 },
                    durationMs = attachment
                        .takeIf { it.has("duration_ms") && !it.isNull("duration_ms") }
                        ?.optLong("duration_ms")
                        ?.takeIf { it >= 0L },
                    filename = attachment
                        .optString("filename")
                        .takeIf { it.isNotBlank() },
                    downloadState = mediaDownloadStateFromWire(
                        status = status,
                        bytesTransferred = download?.optLong("bytes_transferred", 0L) ?: 0L,
                        totalBytes = totalBytes,
                        localPath = localPath,
                        reason = reason,
                    ),
                ),
            )
        }
    }

    private fun parsePollEventId(payload: String): Long? = runCatching {
        val root = JSONObject(payload)
        val body = root.optJSONObject("payload") ?: return@runCatching null
        body.optLong("poll_id").takeIf { it != 0L }
            ?: body.optLong("id").takeIf { it != 0L }
    }.getOrNull()

    private fun parsePollVoteFailure(payload: String): Pair<Long, String>? = runCatching {
        val root = JSONObject(payload)
        val body = root.optJSONObject("payload") ?: return@runCatching null
        val pollId = body.optLong("poll_id")
        val error = body.optString("error")
        if (pollId != 0L && error.isNotBlank()) pollId to error else null
    }.getOrNull()

    private fun completeReactionOperation(type: String, payload: String) {
        val key = parseReactionTargetKey(payload)
        _state.update {
            it.copy(
                pendingReactionKeys = if (key == null) it.pendingReactionKeys else it.pendingReactionKeys - key,
                reactionErrors = if (key == null) it.reactionErrors else it.reactionErrors - key,
                lastEventType = type,
                lastEventPayload = payload,
            )
        }
    }

    private fun parseDeletedMessages(payload: String): Pair<Long?, Set<Long>>? = runCatching {
        val body = JSONObject(payload).optJSONObject("payload") ?: return@runCatching null
        val chatId = if (body.has("chat_id") && !body.isNull("chat_id")) {
            body.optLong("chat_id").takeIf { it != 0L } ?: return@runCatching null
        } else {
            null
        }
        val ids = body.optJSONArray("message_ids") ?: return@runCatching null
        val messageIds = buildSet {
            for (index in 0 until ids.length()) {
                val id = ids.optLong(index)
                if (id > 0L) add(id)
            }
        }
        if (messageIds.isEmpty()) null else chatId to messageIds
    }.getOrNull()

    private fun parseReactionFailure(payload: String): Pair<String, String>? = runCatching {
        val body = JSONObject(payload).optJSONObject("payload") ?: return@runCatching null
        val key = reactionTargetKey(body) ?: return@runCatching null
        val message = body.optString("error").ifBlank { "Не удалось отправить реакцию" }
        key to message
    }.getOrNull()

    private fun parseReactionTargetKey(payload: String): String? = runCatching {
        val body = JSONObject(payload).optJSONObject("payload") ?: return@runCatching null
        reactionTargetKey(body)
    }.getOrNull()

    private fun reactionTargetKey(body: JSONObject): String? {
        val chatId = body.optLong("chat_id")
        val messageId = body.optLong("message_id")
        return if (chatId != 0L && messageId > 0L) "$chatId:$messageId" else null
    }

    private fun parseReactionUpdate(payload: String): ReactionUpdate? = runCatching {
        val body = JSONObject(payload).optJSONObject("payload") ?: return@runCatching null
        val chatId = body.optLong("chat_id")
        val messageId = body.optLong("message_id")
        val reactions = body.optJSONArray("reactions") ?: return@runCatching null
        val revision = parseReactionRevision(body.optJSONObject("revision")) ?: return@runCatching null
        if (chatId == 0L || messageId <= 0L) return@runCatching null
        ReactionUpdate(chatId, messageId, parseReactions(reactions), revision)
    }.getOrNull()

    private fun parseReactionRevision(value: JSONObject?): ReactionRevisionUi? {
        val token = value?.optJSONObject("token") ?: return null
        val generation = token.optLong("generation", -1L)
        val epoch = token.optLong("epoch", -1L)
        val sequence = value.optLong("sequence", -1L)
        if (generation < 0L || epoch < 0L || sequence < 0L) return null
        return ReactionRevisionUi(generation, epoch, sequence)
    }

    private data class ReactionUpdate(
        val chatId: Long,
        val messageId: Long,
        val reactions: List<ConversationReactionUi>,
        val revision: ReactionRevisionUi,
    )

    private fun parseReactions(array: JSONArray?): List<ConversationReactionUi> {
        if (array == null) return emptyList()
        return buildList {
            for (index in 0 until array.length()) {
                val item = array.optJSONObject(index) ?: continue
                val reaction = item.optJSONObject("reaction") ?: continue
                val type = reaction.optString("type")
                if (type != "emoji" && type != "custom_emoji") continue
                val value = reaction.opt("value")?.toString().orEmpty()
                if (value.isBlank()) continue
                add(
                    ConversationReactionUi(
                        type = type,
                        value = value,
                        count = item.optInt("count").coerceAtLeast(0),
                        chosenByMe = item.optBoolean("chosen_by_me"),
                    ),
                )
            }
        }
    }

    private fun parsePinnedMessages(root: JSONObject?): Map<Long, List<Long>> {
        if (root == null) return emptyMap()
        return buildMap {
            val keys = root.keys()
            while (keys.hasNext()) {
                val chatId = keys.next().toLongOrNull() ?: continue
                val ids = root.optJSONArray(chatId.toString()) ?: continue
                put(chatId, (0 until ids.length()).map { ids.optLong(it) }.filter { it > 0L })
            }
        }
    }

    private fun parseReadTarget(payload: String): Pair<Long, Long>? = runCatching {
        val root = JSONObject(payload)
        val event = root.optJSONObject("payload") ?: return@runCatching null
        val chatId = event.optLong("chat_id")
        val messages = event.optJSONArray("messages") ?: return@runCatching null
        var maxMessageId = 0L
        for (index in 0 until messages.length()) {
            val id = messages.optJSONObject(index)?.optLong("id") ?: 0L
            if (id > maxMessageId) maxMessageId = id
        }
        if (chatId != 0L && maxMessageId > 0L) chatId to maxMessageId else null
    }.getOrNull()

    private fun parseAuthorization(value: String): AuthorizationUiStep = when (value) {
        "WaitingForPhoneNumber" -> AuthorizationUiStep.WaitingForPhoneNumber
        "WaitingForCode" -> AuthorizationUiStep.WaitingForCode
        "WaitingForPassword" -> AuthorizationUiStep.WaitingForPassword
        "Ready" -> AuthorizationUiStep.Ready
        "LoggingOut" -> AuthorizationUiStep.LoggingOut
        "Closed" -> AuthorizationUiStep.Closed
        else -> AuthorizationUiStep.Starting
    }

    private fun parseLocation(root: JSONObject?): ConversationLocationUi? {
        if (root == null) return null
        return when (root.optString("kind")) {
            "location" -> parseGeoPoint(root.optJSONObject("point"))
                ?.let { ConversationLocationUi.Point(it) }
            "venue" -> {
                val venue = root.optJSONObject("venue") ?: return null
                val title = venue.optString("title").trim()
                val address = venue.optString("address").trim()
                if (title.isEmpty() || address.isEmpty()) return null
                ConversationLocationUi.Venue(
                    point = parseGeoPoint(venue.optJSONObject("point")),
                    title = title,
                    address = address,
                    provider = venue.optString("provider"),
                    venueId = venue.optString("venue_id"),
                    venueType = venue.optString("venue_type").takeIf(String::isNotEmpty),
                )
            }
            else -> null
        }
    }

    private fun parseGeoPoint(root: JSONObject?): ConversationGeoPointUi? {
        if (root == null || !root.has("latitude") || !root.has("longitude")) return null
        val latitude = root.optDouble("latitude", Double.NaN)
        val longitude = root.optDouble("longitude", Double.NaN)
        if (!latitude.isFinite() || !longitude.isFinite()) return null
        if (latitude !in -90.0..90.0 || longitude !in -180.0..180.0) return null
        val radius = if (root.has("accuracy_radius_meters")) {
            root.optInt("accuracy_radius_meters", -1).takeIf { it in 0..1_500 }
        } else null
        return ConversationGeoPointUi(latitude, longitude, radius)
    }

    private fun formatTimestamp(timestampUnix: Long): String {
        if (timestampUnix <= 0L) return ""
        return runCatching {
            DateTimeFormatter.ofPattern("HH:mm")
                .withZone(ZoneId.systemDefault())
                .format(Instant.ofEpochSecond(timestampUnix))
        }.getOrDefault("")
    }

    override fun onCleared() {
        val currentHandle = lifecycle.close()
        _state.update { it.copy(nativeHandle = 0L) }
        if (currentHandle != null && NativeLibrary.isLoaded) {
            runCatching { NativeBridge.destroyHandle(currentHandle) }
        }
        super.onCleared()
    }
}
