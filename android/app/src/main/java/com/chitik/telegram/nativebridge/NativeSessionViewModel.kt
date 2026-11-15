package com.chitik.telegram.nativebridge

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.chitik.telegram.BuildConfig
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

enum class AuthorizationUiState {
    Closed,
    WaitingForPhoneNumber,
    WaitingForCode,
    WaitingForPassword,
    Ready,
    LoggingOut,
    Unknown,
}

data class ChatUi(
    val id: Long,
    val title: String,
    val unreadCount: Int,
    val lastMessage: String?,
)

data class MessageUi(
    val id: Long,
    val chatId: Long,
    val senderId: Long,
    val text: String,
    val timestampUnix: Long,
    val outgoing: Boolean,
)

data class NativeUiState(
    val libraryLoaded: Boolean = false,
    val coreVersion: String = "Not loaded",
    val nativeHandle: Long = 0L,
    val telegramConfigured: Boolean = false,
    val authorization: AuthorizationUiState = AuthorizationUiState.Closed,
    val chats: List<ChatUi> = emptyList(),
    val messages: Map<Long, List<MessageUi>> = emptyMap(),
    val lastEventType: String = "none",
    val lastEventPayload: String = "",
    val busy: Boolean = false,
    val error: String? = null,
) {
    val coreReady: Boolean
        get() = libraryLoaded && nativeHandle != 0L && error == null
}

class NativeSessionViewModel(application: Application) :
    AndroidViewModel(application),
    NativeEventCallback {

    private val _state = MutableStateFlow(NativeUiState())
    val state: StateFlow<NativeUiState> = _state.asStateFlow()

    @Volatile
    private var handle: Long = 0L

    private val sessionPath: String
        get() {
            val directory = File(getApplication<Application>().filesDir, "telegram")
            directory.mkdirs()
            return File(directory, "session.sqlite").absolutePath
        }

    init {
        initializeNativeSession()
    }

    private fun initializeNativeSession() {
        if (!NativeLibrary.isLoaded) {
            _state.value = NativeUiState(
                libraryLoaded = false,
                error = NativeLibrary.errorMessage ?: "Native library is unavailable",
            )
            return
        }

        _state.update { it.copy(libraryLoaded = true, busy = true, error = null) }

        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val version = NativeBridge.coreVersion()
                val configured = BuildConfig.TELEGRAM_API_ID > 0 &&
                    BuildConfig.TELEGRAM_API_HASH.isNotBlank()
                val newHandle = if (configured) {
                    NativeBridge.createTelegramHandle(
                        this@NativeSessionViewModel,
                        BuildConfig.TELEGRAM_API_ID,
                        BuildConfig.TELEGRAM_API_HASH,
                        sessionPath,
                    )
                } else {
                    NativeBridge.createHandle(this@NativeSessionViewModel)
                }
                require(newHandle != 0L) { "Rust returned an invalid native handle" }
                handle = newHandle
                NativeBridge.probe(newHandle)
                Triple(version, newHandle, configured)
            }.onSuccess { (version, newHandle, configured) ->
                _state.update { current ->
                    current.copy(
                        libraryLoaded = true,
                        coreVersion = version,
                        nativeHandle = newHandle,
                        telegramConfigured = configured,
                        busy = false,
                        error = null,
                    )
                }
                if (configured) {
                    sendStartCommand()
                }
            }.onFailure { error ->
                _state.update { current ->
                    current.copy(
                        libraryLoaded = true,
                        nativeHandle = handle,
                        busy = false,
                        error = error.message ?: error::class.java.simpleName,
                    )
                }
            }
        }
    }

    fun configureTelegram(apiIdText: String, apiHash: String) {
        val apiId = apiIdText.trim().toIntOrNull()
        if (apiId == null || apiId <= 0 || apiHash.isBlank()) {
            _state.update { it.copy(error = "Enter a valid Telegram API ID and API hash") }
            return
        }

        _state.update { it.copy(busy = true, error = null) }
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val oldHandle = handle
                handle = 0L
                if (oldHandle != 0L) {
                    NativeBridge.destroyHandle(oldHandle)
                }

                val newHandle = NativeBridge.createTelegramHandle(
                    this@NativeSessionViewModel,
                    apiId,
                    apiHash.trim(),
                    sessionPath,
                )
                require(newHandle != 0L) { "Rust returned an invalid Telegram handle" }
                handle = newHandle
                NativeBridge.probe(newHandle)
                newHandle
            }.onSuccess { newHandle ->
                _state.update { current ->
                    current.copy(
                        nativeHandle = newHandle,
                        telegramConfigured = true,
                        authorization = AuthorizationUiState.Closed,
                        chats = emptyList(),
                        busy = false,
                        error = null,
                    )
                }
                sendStartCommand()
            }.onFailure { error ->
                _state.update {
                    it.copy(
                        nativeHandle = 0L,
                        telegramConfigured = false,
                        busy = false,
                        error = error.message ?: error::class.java.simpleName,
                    )
                }
            }
        }
    }

    fun probeRoundTrip() {
        val currentHandle = handle
        if (currentHandle == 0L || !NativeLibrary.isLoaded) {
            _state.update { it.copy(error = "Native bridge is not available") }
            return
        }

        runCatching { NativeBridge.probe(currentHandle) }
            .onFailure { error ->
                _state.update {
                    it.copy(error = error.message ?: error::class.java.simpleName)
                }
            }
    }

    fun sendStartCommand() {
        sendCommand(JSONObject().put("type", "start").toString())
    }

    fun submitPhoneNumber(phoneNumber: String) {
        sendCommand(
            JSONObject()
                .put("type", "set_phone_number")
                .put("payload", phoneNumber.trim())
                .toString(),
        )
    }

    fun submitCode(code: String) {
        sendCommand(
            JSONObject()
                .put("type", "submit_code")
                .put("payload", code.trim())
                .toString(),
        )
    }

    fun submitPassword(password: String) {
        sendCommand(
            JSONObject()
                .put("type", "submit_password")
                .put("payload", password)
                .toString(),
        )
    }

    fun loadChats(limit: Int = 100) {
        sendCommand(
            JSONObject()
                .put("type", "load_chats")
                .put("payload", JSONObject().put("limit", limit))
                .toString(),
        )
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

    private fun markRead(chatId: Long, maxMessageId: Long) {
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

    private fun sendCommand(commandJson: String) {
        val currentHandle = handle
        if (currentHandle == 0L || !NativeLibrary.isLoaded) {
            _state.update { it.copy(error = "Native bridge is not available") }
            return
        }

        runCatching { NativeBridge.sendCommand(currentHandle, commandJson) }
            .onSuccess { accepted ->
                if (!accepted) {
                    _state.update { it.copy(error = "Rust rejected the command") }
                }
            }
            .onFailure { error ->
                _state.update {
                    it.copy(error = error.message ?: error::class.java.simpleName)
                }
            }
    }

    override fun onNativeEvent(type: String, payload: String) {
        when (type) {
            "state" -> applyStatePayload(payload)
            "authorization_changed" -> applyAuthorizationPayload(payload)
            "chats_loaded" -> applyChatsEvent(payload)
            "messages_loaded" -> {
                val readTarget = runCatching {
                    val eventPayload = JSONObject(payload).optJSONObject("payload")
                    val chatId = eventPayload?.optLong("chat_id") ?: 0L
                    val messages = eventPayload?.optJSONArray("messages")
                    var maxMessageId = 0L
                    if (messages != null) {
                        for (index in 0 until messages.length()) {
                            val messageId = messages.optJSONObject(index)?.optLong("id") ?: 0L
                            if (messageId > maxMessageId) {
                                maxMessageId = messageId
                            }
                        }
                    }
                    chatId to maxMessageId
                }.getOrDefault(0L to 0L)

                _state.update {
                    it.copy(
                        lastEventType = type,
                        lastEventPayload = payload,
                        busy = false,
                        error = null,
                    )
                }

                if (readTarget.first != 0L && readTarget.second > 0L) {
                    markRead(readTarget.first, readTarget.second)
                }
            }
            "error" -> {
                val message = runCatching {
                    JSONObject(payload).optString("message", payload)
                }.getOrDefault(payload)
                _state.update {
                    it.copy(
                        lastEventType = type,
                        lastEventPayload = payload,
                        busy = false,
                        error = message,
                    )
                }
            }
            else -> {
                _state.update { current ->
                    current.copy(
                        lastEventType = type,
                        lastEventPayload = payload,
                    )
                }
            }
        }
    }

    private fun applyAuthorizationPayload(payload: String) {
        val authorization = runCatching {
            parseAuthorization(JSONObject(payload).optString("payload"))
        }.getOrDefault(AuthorizationUiState.Unknown)

        _state.update {
            it.copy(
                authorization = authorization,
                lastEventType = "authorization_changed",
                lastEventPayload = payload,
                busy = false,
                error = null,
            )
        }

        if (authorization == AuthorizationUiState.Ready) {
            loadChats()
        }
    }

    private fun applyChatsEvent(payload: String) {
        val chats = runCatching {
            parseChats(JSONObject(payload).optJSONArray("payload") ?: JSONArray())
        }.getOrDefault(emptyList())

        _state.update {
            it.copy(
                chats = chats,
                lastEventType = "chats_loaded",
                lastEventPayload = payload,
                busy = false,
                error = null,
            )
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
                    lastEventType = "state",
                    lastEventPayload = payload,
                    busy = false,
                )
            }
        }.onFailure { error ->
            _state.update {
                it.copy(error = "Invalid Rust state payload: ${error.message}")
            }
        }
    }

    private fun parseChats(array: JSONArray): List<ChatUi> = buildList {
        for (index in 0 until array.length()) {
            val chat = array.optJSONObject(index) ?: continue
            val lastMessage = chat.optJSONObject("last_message")
            add(
                ChatUi(
                    id = chat.optLong("id"),
                    title = chat.optString("title", "Chat"),
                    unreadCount = chat.optInt("unread_count", 0),
                    lastMessage = lastMessage?.optString("text")?.takeIf { it.isNotBlank() },
                ),
            )
        }
    }

    private fun parseMessages(root: JSONObject?): Map<Long, List<MessageUi>> {
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
                        add(
                            MessageUi(
                                id = message.optLong("id"),
                                chatId = message.optLong("chat_id", chatId),
                                senderId = message.optLong("sender_id"),
                                text = message.optString("text"),
                                timestampUnix = message.optLong("timestamp_unix"),
                                outgoing = message.optBoolean("outgoing"),
                            ),
                        )
                    }
                }
                put(chatId, messages)
            }
        }
    }

    private fun parseAuthorization(value: String): AuthorizationUiState = when (value) {
        "WaitingForPhoneNumber" -> AuthorizationUiState.WaitingForPhoneNumber
        "WaitingForCode" -> AuthorizationUiState.WaitingForCode
        "WaitingForPassword" -> AuthorizationUiState.WaitingForPassword
        "Ready" -> AuthorizationUiState.Ready
        "LoggingOut" -> AuthorizationUiState.LoggingOut
        "Closed" -> AuthorizationUiState.Closed
        else -> AuthorizationUiState.Unknown
    }

    override fun onCleared() {
        val currentHandle = handle
        handle = 0L
        if (currentHandle != 0L && NativeLibrary.isLoaded) {
            runCatching { NativeBridge.destroyHandle(currentHandle) }
        }
        super.onCleared()
    }
}
