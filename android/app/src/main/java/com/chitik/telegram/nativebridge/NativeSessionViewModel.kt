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
 * Owns one JNI session for the lifetime of this ViewModelStore owner.
 *
 * Android retains this ViewModel across Activity recreation and while the app is
 * backgrounded. Permanent owner cleanup closes the lifecycle gate once and asks
 * Rust to shut down the session. The native session store is responsible for
 * re-opening persisted Telegram authorization after a process restart.
 */
class NativeSessionViewModel : ViewModel(), NativeEventCallback {
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

        val creationToken = lifecycle.beginHandleCreation() ?: return
        var createdHandle = 0L
        runCatching {
            val version = NativeBridge.coreVersion()
            createdHandle = NativeBridge.createHandle(this)
            require(createdHandle != 0L) { "Rust returned an invalid native handle" }

            if (!lifecycle.installHandle(creationToken, createdHandle)) {
                runCatching { NativeBridge.destroyHandle(createdHandle) }
                createdHandle = 0L
                throw IllegalStateException("Native session owner was already cleared")
            }

            val probeResult = lifecycle.dispatchCommand { currentHandle ->
                NativeBridge.probe(currentHandle)
            }
            if (probeResult != LifecycleDispatchResult.Accepted) {
                throw IllegalStateException("Native bridge probe was not accepted: $probeResult")
            }
            version
        }.onSuccess { version ->
            lifecycle.runIfOpen {
                _state.update { current ->
                    current.copy(
                        libraryLoaded = true,
                        coreVersion = version,
                        nativeHandle = createdHandle,
                        error = null,
                    )
                }
            }
        }.onFailure { error ->
            lifecycle.failHandleCreation(creationToken)
            lifecycle.runIfOpen {
                _state.update { current ->
                    current.copy(
                        libraryLoaded = true,
                        nativeHandle = lifecycle.currentHandleOrZero(),
                        error = error.message ?: error::class.java.simpleName,
                    )
                }
            }
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

        reportDispatchResult(result, rejectedMessage = "Rust rejected the JNI probe")
    }

    fun sendStartCommand() {
        if (!NativeLibrary.isLoaded) {
            setErrorIfOpen("Native bridge is not available")
            return
        }

        val result = runCatching {
            lifecycle.dispatchStart { currentHandle ->
                NativeBridge.sendCommand(currentHandle, """{"type":"start"}""")
            }
        }.getOrElse { error ->
            setErrorIfOpen(error.message ?: error::class.java.simpleName)
            return
        }

        reportDispatchResult(result, rejectedMessage = "Rust rejected the command")
    }

    fun loadChats(limit: Int = 100) {
        val safeLimit = limit.coerceIn(1, 500)
        sendCoalescedLoadChats(safeLimit)
    }

    private fun sendCoalescedLoadChats(limit: Int) {
        if (!NativeLibrary.isLoaded) {
            setErrorIfOpen("Native bridge is not available")
            return
        }

        val commandJson = """{"type":"load_chats","payload":{"limit":$limit}}"""
        val result = runCatching {
            lifecycle.dispatchLoadChats { currentHandle ->
                NativeBridge.sendCommand(currentHandle, commandJson)
            }
        }.getOrElse { error ->
            setErrorIfOpen(error.message ?: error::class.java.simpleName)
            return
        }

        reportDispatchResult(result, rejectedMessage = "Rust rejected the chat request")
    }

    private fun reportDispatchResult(
        result: LifecycleDispatchResult,
        rejectedMessage: String,
    ) {
        when (result) {
            LifecycleDispatchResult.Accepted,
            LifecycleDispatchResult.Duplicate,
            LifecycleDispatchResult.Closed,
            -> Unit

            LifecycleDispatchResult.NoHandle ->
                setErrorIfOpen("Native bridge is not available")

            LifecycleDispatchResult.Rejected ->
                setErrorIfOpen(rejectedMessage)
        }
    }

    private fun setErrorIfOpen(message: String) {
        lifecycle.runIfOpen {
            _state.update { it.copy(error = message) }
        }
    }

    override fun onNativeEvent(type: String, payload: String) {
        if (!lifecycle.onNativeEvent(type)) return

        lifecycle.runIfOpen {
            _state.update { current ->
                current.copy(
                    lastEventType = type,
                    lastEventPayload = payload,
                    error = if (type == "error") payload else current.error,
                )
            }
        }
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
