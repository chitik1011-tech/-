package com.chitik.telegram.nativebridge

internal data class NativeHandleCreation(
    val generation: Long,
    val detachedHandle: Long?,
)

/**
 * Serializes ownership of the JNI handle across creation, replacement, command dispatch and
 * permanent ViewModel cleanup. Every native callback is tagged with the generation that created
 * its handle, so callbacks from a destroyed/replaced session cannot mutate the new session.
 */
internal class NativeSessionLifecycle {
    private val lock = Any()
    private var closed = false
    private var nextGeneration = 0L
    private var pendingGeneration: Long? = null
    private var activeGeneration: Long? = null
    private var handle = 0L
    private var startPending = false
    private var startSent = false
    private var chatsPending = false

    /**
     * Starts initial creation or replacement. Existing ownership is detached atomically; the
     * caller must destroy [NativeHandleCreation.detachedHandle] outside the lock before creating
     * the replacement.
     */
    fun beginReplacement(): NativeHandleCreation? = synchronized(lock) {
        if (closed || pendingGeneration != null) return@synchronized null

        nextGeneration += 1
        val generation = nextGeneration
        pendingGeneration = generation
        activeGeneration = null
        resetCommandFlags()

        val detached = handle.also { handle = 0L }.takeIf { it != 0L }
        NativeHandleCreation(generation, detached)
    }

    /**
     * Installs a newly created handle if this creation is still current. False means the caller
     * must immediately destroy [nativeHandle].
     */
    fun installHandle(generation: Long, nativeHandle: Long): Boolean = synchronized(lock) {
        if (
            closed ||
            nativeHandle == 0L ||
            pendingGeneration != generation ||
            handle != 0L
        ) {
            return@synchronized false
        }

        pendingGeneration = null
        activeGeneration = generation
        handle = nativeHandle
        true
    }

    fun failCreation(generation: Long) = synchronized(lock) {
        if (pendingGeneration == generation) {
            pendingGeneration = null
        }
    }

    fun currentHandleOrZero(): Long = synchronized(lock) {
        if (closed) 0L else handle
    }

    fun dispatchCommand(send: (Long) -> Boolean): LifecycleDispatchResult = synchronized(lock) {
        dispatchWithHandle(send)
    }

    fun dispatchStart(send: (Long) -> Boolean): LifecycleDispatchResult = synchronized(lock) {
        if (closed) return@synchronized LifecycleDispatchResult.Closed
        if (handle == 0L) return@synchronized LifecycleDispatchResult.NoHandle
        if (startPending || startSent) return@synchronized LifecycleDispatchResult.Duplicate

        startPending = true
        try {
            dispatchWithHandle(send).also { result ->
                if (result != LifecycleDispatchResult.Accepted) startPending = false
            }
        } catch (error: Throwable) {
            startPending = false
            throw error
        }
    }

    fun dispatchLoadChats(send: (Long) -> Boolean): LifecycleDispatchResult = synchronized(lock) {
        if (closed) return@synchronized LifecycleDispatchResult.Closed
        if (handle == 0L) return@synchronized LifecycleDispatchResult.NoHandle
        if (chatsPending) return@synchronized LifecycleDispatchResult.Duplicate

        chatsPending = true
        try {
            dispatchWithHandle(send).also { result ->
                if (result != LifecycleDispatchResult.Accepted) chatsPending = false
            }
        } catch (error: Throwable) {
            chatsPending = false
            throw error
        }
    }

    /**
     * Accepts callbacks only from the current pending/active generation. Pending callbacks are
     * allowed because Rust may emit its initial state before createHandle returns to Kotlin.
     */
    fun onNativeEvent(generation: Long, type: String): Boolean = synchronized(lock) {
        if (closed) return@synchronized false
        if (generation != activeGeneration && generation != pendingGeneration) {
            return@synchronized false
        }

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
            "stopped" -> resetCommandFlags()
        }
        true
    }

    fun runIfCurrent(generation: Long, action: () -> Unit): Boolean = synchronized(lock) {
        if (
            closed ||
            (generation != activeGeneration && generation != pendingGeneration)
        ) {
            false
        } else {
            action()
            true
        }
    }

    fun runIfOpen(action: () -> Unit): Boolean = synchronized(lock) {
        if (closed) false else {
            action()
            true
        }
    }

    /** Permanently closes this owner and transfers the current handle for JNI destruction. */
    fun close(): Long? = synchronized(lock) {
        if (closed) return@synchronized null

        closed = true
        pendingGeneration = null
        activeGeneration = null
        resetCommandFlags()
        handle.also { handle = 0L }.takeIf { it != 0L }
    }

    private fun dispatchWithHandle(send: (Long) -> Boolean): LifecycleDispatchResult {
        if (closed) return LifecycleDispatchResult.Closed
        if (handle == 0L || activeGeneration == null) return LifecycleDispatchResult.NoHandle
        return if (send(handle)) {
            LifecycleDispatchResult.Accepted
        } else {
            LifecycleDispatchResult.Rejected
        }
    }

    private fun resetCommandFlags() {
        startPending = false
        startSent = false
        chatsPending = false
    }
}

internal enum class LifecycleDispatchResult {
    Accepted,
    Duplicate,
    Closed,
    NoHandle,
    Rejected,
}
