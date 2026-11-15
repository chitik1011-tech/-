package com.chitik.telegram.nativebridge

/**
 * Serializes ownership of the native session handle and its command lifecycle.
 *
 * The ViewModelStore owner keeps this gate alive through Activity recreation and
 * while the app is backgrounded. Permanent owner cleanup closes it once; late
 * creations and callbacks are rejected.
 */
internal class NativeSessionLifecycle {
    private val lock = Any()
    private var closed = false
    private var nextCreationToken = 0L
    private var activeCreationToken: Long? = null
    private var handle = 0L
    private var startPending = false
    private var startSent = false
    private var chatsPending = false

    fun beginHandleCreation(): Long? = synchronized(lock) {
        if (closed || handle != 0L || activeCreationToken != null) {
            return@synchronized null
        }

        nextCreationToken += 1
        activeCreationToken = nextCreationToken
        nextCreationToken
    }

    /**
     * Installs a handle only if this owner is still open and the creation is current.
     * A false result means the caller must destroy the newly created native handle.
     */
    fun installHandle(creationToken: Long, nativeHandle: Long): Boolean = synchronized(lock) {
        if (
            closed ||
            nativeHandle == 0L ||
            handle != 0L ||
            activeCreationToken != creationToken
        ) {
            return@synchronized false
        }

        activeCreationToken = null
        handle = nativeHandle
        true
    }

    fun failHandleCreation(creationToken: Long) = synchronized(lock) {
        if (activeCreationToken == creationToken) {
            activeCreationToken = null
        }
    }

    /** Sends a non-coalesced command while excluding concurrent handle teardown. */
    fun dispatchCommand(send: (Long) -> Boolean): LifecycleDispatchResult =
        synchronized(lock) {
            dispatchWithHandle(send)
        }

    /** Allows at most one Start until authorization changes or an error arrives. */
    fun dispatchStart(send: (Long) -> Boolean): LifecycleDispatchResult =
        synchronized(lock) {
            if (closed) return@synchronized LifecycleDispatchResult.Closed
            if (handle == 0L) return@synchronized LifecycleDispatchResult.NoHandle
            if (startPending || startSent) {
                return@synchronized LifecycleDispatchResult.Duplicate
            }

            startPending = true
            dispatchWithHandle(send).also { result ->
                if (result != LifecycleDispatchResult.Accepted) {
                    startPending = false
                }
            }
        }

    /** Coalesces duplicate chat loads until ChatsLoaded or an error is reported. */
    fun dispatchLoadChats(send: (Long) -> Boolean): LifecycleDispatchResult =
        synchronized(lock) {
            if (closed) return@synchronized LifecycleDispatchResult.Closed
            if (handle == 0L) return@synchronized LifecycleDispatchResult.NoHandle
            if (chatsPending) return@synchronized LifecycleDispatchResult.Duplicate

            chatsPending = true
            dispatchWithHandle(send).also { result ->
                if (result != LifecycleDispatchResult.Accepted) {
                    chatsPending = false
                }
            }
        }

    /**
     * Returns false for late callbacks after owner cleanup. Relevant in-flight
     * command flags are released so a failed request can be retried.
     */
    fun onNativeEvent(type: String): Boolean = synchronized(lock) {
        if (closed) return@synchronized false

        when (type) {
            "authorization_changed" -> {
                startPending = false
                startSent = true
            }
            "chats_loaded" -> chatsPending = false
            "error" -> {
                startPending = false
                startSent = false
                chatsPending = false
            }
            "stopped" -> {
                startPending = false
                startSent = false
                chatsPending = false
            }
        }
        true
    }

    fun acceptsCallbacks(): Boolean = synchronized(lock) { !closed }

    /** Closes once and transfers the owned handle to the caller for JNI cleanup. */
    fun close(): Long? = synchronized(lock) {
        if (closed) return@synchronized null

        closed = true
        activeCreationToken = null
        startPending = false
        startSent = false
        chatsPending = false
        handle.also { handle = 0L }.takeIf { it != 0L }
    }

    private fun dispatchWithHandle(send: (Long) -> Boolean): LifecycleDispatchResult {
        if (closed) return LifecycleDispatchResult.Closed
        if (handle == 0L) return LifecycleDispatchResult.NoHandle
        return if (send(handle)) {
            LifecycleDispatchResult.Accepted
        } else {
            LifecycleDispatchResult.Rejected
        }
    }
}

internal enum class LifecycleDispatchResult {
    Accepted,
    Duplicate,
    Closed,
    NoHandle,
    Rejected,
}
