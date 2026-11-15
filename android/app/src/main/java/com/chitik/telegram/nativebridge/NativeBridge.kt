package com.chitik.telegram.nativebridge

interface NativeEventCallback {
    fun onNativeEvent(type: String, payload: String)
}

object NativeBridge {
    external fun coreVersion(): String
    external fun createHandle(callback: NativeEventCallback): Long
    external fun sendCommand(handle: Long, commandJson: String): Boolean
    external fun probe(handle: Long): Boolean
    external fun destroyHandle(handle: Long)
}
