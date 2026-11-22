package com.chitik.telegram.nativebridge

/** Prevents ordinary Kotlin failures from escaping across the Rust-to-JVM callback boundary. */
internal inline fun dispatchNativeCallbackSafely(
    dispatch: () -> Unit,
    onFailure: (Exception) -> Unit,
) {
    try {
        dispatch()
    } catch (error: Exception) {
        // Callback reporting itself must not leave a Java exception pending in the JNI caller.
        try {
            onFailure(error)
        } catch (_: Exception) {
            // A secondary ordinary failure is contained; fatal Errors still escape.
        }
    }
}
