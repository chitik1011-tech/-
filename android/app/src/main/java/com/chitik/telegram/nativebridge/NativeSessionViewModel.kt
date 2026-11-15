package com.chitik.telegram.nativebridge

import androidx.lifecycle.ViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

data class NativeUiState(
    val libraryLoaded: Boolean = false,
    val coreVersion: String = "Not loaded",
    val nativeHandle: Long = 0L,
    val lastEventType: String = "none",
    val lastEventPayload: String = "",
    val error: String? = null,
) {
    val coreReady: Boolean
        get() = libraryLoaded && nativeHandle != 0L && error == null
}

/**
 * Owns one JNI handle for the lifetime of its ViewModelStore owner.
 *
 * Activity recreation keeps this ViewModel and its native handle. When the owner is
 * permanently cleared, the handle is destroyed once. A process restart creates a new
 * handle; durable Telegram session restoration is provided by the Rust backend.
 */
class NativeSessionViewModel : ViewModel(), NativeEventCallback {
    private val _state = MutableStateFlow(NativeUiState())
    val state: StateFlow<NativeUiState> = _state.asStateFlow()

    private val lifecycleLock = Any()
    private var handle: Long = 0L
    private var cleared = false

    init {
        initializeNativeSession()
    }

    private fun initializeNativeSession() {
        if (!NativeLibrary.isLoaded) {
            updateStateIfOpen {
                it.copy(
                    libraryLoaded = false,
                    error = NativeLibrary.errorMessage ?: "Native library is unavailable",
                )
            }
            return
        }

        var createdHandle = 0L
        runCatching {
            val version = NativeBridge.coreVersion()
            createdHandle = NativeBridge.createHandle(this)
            require(createdHandle != 0L) { "Rust returned an invalid native handle" }

            val registered = synchronized(lifecycleLock) {
                if (cleared) {
                    false
                } else {
                    handle = createdHandle
                    true
                }
            }
            if (!registered) {
                runCatching { NativeBridge.destroyHandle(createdHandle) }
                throw IllegalStateException("Native session owner was already cleared")
            }

            NativeBridge.probe(createdHandle)
            version
        }.onSuccess { version ->
            updateStateIfOpen {
                it.copy(
                    libraryLoaded = true,
                    coreVersion = version,
                    nativeHandle = createdHandle,
                    error = null,
                )
            }
        }.onFailure { error ->
            updateStateIfOpen {
                it.copy(
                    libraryLoaded = true,
                    nativeHandle = currentHandle(),
                    error = error.message ?: error::class.java.simpleName,
                )
            }
        }
    }

    fun probeRoundTrip() {
        withActiveHandle { currentHandle ->
            runCatching { NativeBridge.probe(currentHandle) }
                .onFailure { error ->
                    _state.update {
                        it.copy(error = error.message ?: error::class.java.simpleName)
                    }
                }
        }
    }

    fun sendStartCommand() {
        sendCommand("""{"type":"start"}""")
    }

    private fun sendCommand(commandJson: String) {
        withActiveHandle { currentHandle ->
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
    }

    override fun onNativeEvent(type: String, payload: String) {
        updateStateIfOpen { current ->
            current.copy(
                lastEventType = type,
                lastEventPayload = payload,
                error = if (type == "error") payload else current.error,
            )
        }
    }

    /**
     * Runs a JNI operation while cleanup is excluded, so a command cannot race with
     * handle destruction. Native network work remains asynchronous inside Rust.
     */
    private fun withActiveHandle(action: (Long) -> Unit) {
        synchronized(lifecycleLock) {
            if (cleared) return

            val currentHandle = handle
            if (currentHandle == 0L || !NativeLibrary.isLoaded) {
                _state.update { it.copy(error = "Native bridge is not available") }
                return
            }

            action(currentHandle)
        }
    }

    private fun currentHandle(): Long = synchronized(lifecycleLock) { handle }

    private fun updateStateIfOpen(reducer: (NativeUiState) -> NativeUiState) {
        synchronized(lifecycleLock) {
            if (!cleared) {
                _state.update(reducer)
            }
        }
    }

    override fun onCleared() {
        val currentHandle = synchronized(lifecycleLock) {
            if (cleared) {
                0L
            } else {
                cleared = true
                val existing = handle
                handle = 0L
                existing
            }
        }

        if (currentHandle != 0L && NativeLibrary.isLoaded) {
            runCatching { NativeBridge.destroyHandle(currentHandle) }
        }
        super.onCleared()
    }
}
