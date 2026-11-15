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

class NativeSessionViewModel : ViewModel(), NativeEventCallback {
    private val _state = MutableStateFlow(NativeUiState())
    val state: StateFlow<NativeUiState> = _state.asStateFlow()

    private var handle: Long = 0L

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

        runCatching {
            val version = NativeBridge.coreVersion()
            val newHandle = NativeBridge.createHandle(this)
            require(newHandle != 0L) { "Rust returned an invalid native handle" }
            handle = newHandle
            NativeBridge.probe(newHandle)
            version
        }.onSuccess { version ->
            _state.update { current ->
                current.copy(
                    libraryLoaded = true,
                    coreVersion = version,
                    nativeHandle = handle,
                    error = null,
                )
            }
        }.onFailure { error ->
            _state.update { current ->
                current.copy(
                    libraryLoaded = true,
                    nativeHandle = handle,
                    error = error.message ?: error::class.java.simpleName,
                )
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
        sendCommand("""{"type":"start"}""")
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
        _state.update { current ->
            current.copy(
                lastEventType = type,
                lastEventPayload = payload,
                error = if (type == "error") payload else current.error,
            )
        }
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
