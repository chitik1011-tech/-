package com.chitik.telegram.nativebridge

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.chitik.telegram.AuthorizationUiStep
import com.chitik.telegram.ChatListEntry
import com.chitik.telegram.ChatListPreview
import com.chitik.telegram.ConversationMessageUi
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
    val messages: Map<Long, List<ConversationMessageUi>> = emptyMap(),
    val busy: Boolean = false,
    val lastEventType: String = "none",
    val lastEventPayload: String = "",
    val error: String? = null,
) {
    val coreReady: Boolean
        get() = libraryLoaded && nativeHandle != 0L && error == null
}

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

    fun clearTelegramConfig() {
        _state.update { it.copy(busy = true, error = null) }

        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                TelegramRuntimeConfigStore.clear(getApplication())
                replaceNativeHandle(config = null)
            }.onSuccess { newHandle ->
                lifecycle.runIfOpen {
                    _state.update {
                        it.copy(
                            nativeHandle = newHandle,
                            telegramConfigured = false,
                            authorization = AuthorizationUiStep.Closed,
                            chats = emptyList(),
                            messages = emptyMap(),
                            busy = false,
                            error = null,
                        )
                    }
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

    fun loadMessages(chatId: Long, limit: Int = 100) {
        _state.update { it.copy(busy = true, error = null) }
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

    fun sendText(chatId: Long, text: String) {
        sendCommand(
            JSONObject()
                .put("type", "send_text")
                .put(
                    "payload",
                    JSONObject()
                        .put("chat_id", chatId)
                        .put("text", text),
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

    private fun sendCommand(commandJson: String) {
        if (!NativeLibrary.isLoaded) {
            setErrorIfOpen("Native bridge is not available")
            return
        }

        val result = runCatching {
            lifecycle.dispatchCommand { currentHandle ->
                NativeBridge.sendCommand(currentHandle, commandJson)
            }
        }.getOrElse { error ->
            setErrorIfOpen(error.message ?: error::class.java.simpleName)
            return
        }

        reportDispatchResult(result, "Rust rejected the command")
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
                    error = message,
                )
            }
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
            "state" -> applyStatePayload(payload)
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
            "messages_loaded" -> {
                val target = parseReadTarget(payload)
                _state.update {
                    it.copy(
                        busy = false,
                        lastEventType = type,
                        lastEventPayload = payload,
                        error = null,
                    )
                }
                target?.let { (chatId, maxMessageId) ->
                    markRead(chatId, maxMessageId)
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

        _state.update {
            it.copy(
                authorization = authorization,
                busy = false,
                lastEventType = "authorization_changed",
                lastEventPayload = payload,
                error = null,
            )
        }

        if (authorization == AuthorizationUiStep.Ready) {
            loadChats()
        }
    }

    private fun applyStatePayload(payload: String) {
        runCatching {
            val root = JSONObject(payload)
            val account = root.getJSONObject("account")
            val authorization = parseAuthorization(account.optString("authorization"))
            val chats = parseChats(account.optJSONArray("chats") ?: JSONArray())
            val messages = parseMessages(root.optJSONObject("messages"))
            Triple(authorization, chats, messages)
        }.onSuccess { (authorization, chats, messages) ->
            _state.update {
                it.copy(
                    authorization = authorization,
                    chats = chats,
                    messages = messages,
                    busy = false,
                    lastEventType = "state",
                    lastEventPayload = payload,
                    error = null,
                )
            }
        }.onFailure { error ->
            _state.update {
                it.copy(
                    busy = false,
                    error = "Invalid Rust state payload: ${error.message}",
                )
            }
        }
    }

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

    private fun parseMessages(root: JSONObject?): Map<Long, List<ConversationMessageUi>> {
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
                        add(
                            ConversationMessageUi(
                                id = message.optLong("id").toString(),
                                chatId = chatId.toString(),
                                text = message.optString("text"),
                                timestamp = formatTimestamp(timestamp),
                                isOutgoing = message.optBoolean("outgoing"),
                            ),
                        )
                    }
                }
                put(chatId, messages)
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
