package com.chitik.telegram.nativebridge

object NativeLibrary {
    private val loadResult: Result<Unit> = runCatching {
        System.loadLibrary("tg_android_bridge")
    }

    val isLoaded: Boolean
        get() = loadResult.isSuccess

    val errorMessage: String?
        get() = loadResult.exceptionOrNull()?.let { error ->
            "\${error::class.java.simpleName}: \${error.message ?: "native library load failed"}"
        }
}
