# Android shell + Rust JNI bridge

This directory contains the Android/Compose shell for the Rust-first Telegram client. The APK is ARM64-first and loads \`libtg_android_bridge.so\` with \`System.loadLibrary("tg_android_bridge")\`.

## Requirements

- JDK 17 or newer supported by Gradle/AGP
- Android SDK Platform 37
- Android NDK (AGP 9.5 default NDK 28.2.13676358 is supported)
- Rust toolchain with \`rustup\`
- \`cargo-ndk\` (the build script installs it automatically when missing)

Set the SDK/NDK environment variables, for example:

~~~bash
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
~~~

## Build

From the repository root:

~~~bash
rustup target add aarch64-linux-android
cargo install cargo-ndk --locked   # optional; android/build-rust.sh installs it if missing
cargo check --workspace
cd android
./gradlew assembleDebug
~~~

\`preBuild\` runs \`android/build-rust.sh\`, which compiles the \`tg-android-bridge\` crate with \`cargo-ndk\` and writes:

~~~text
android/app/src/main/jniLibs/arm64-v8a/libtg_android_bridge.so
~~~

The debug APK is produced at:

~~~text
android/app/build/outputs/apk/debug/app-debug.apk
~~~

## JNI verification

Launch the app and open the **Core** screen.

1. **Native library** must show \`Loaded\`.
2. **Core version** is returned by Rust from \`coreVersion()\`; there is no Kotlin fallback implementation.
3. Press **JNI round-trip**. Android calls \`NativeBridge.probe(handle)\`, Rust receives the JNI call, then invokes \`NativeEventCallback.onNativeEvent(...)\` back on Android. The latest event should become \`jni_probe\` with \`{"round_trip":true}\`.
4. Press **CoreCommand::Start** to exercise Android → JNI → \`CoreHandle::send\` → \`CoreEvent\` → JNI callback. Until the protocol agent provides a concrete Telegram backend, the intentionally disconnected backend returns an explicit backend error rather than pretending Telegram is connected.

## Native handle lifecycle

\`NativeSessionViewModel\` owns the opaque native handle. Activity recreation reuses the ViewModel and therefore does not recreate the Rust session. When the ViewModel is permanently cleared, \`destroyHandle()\` removes the registry entry and sends \`CoreCommand::Shutdown\`.

Library loading and all JNI calls are wrapped in Kotlin \`runCatching\`; JNI entry points catch Rust panics and translate bridge errors into Java exceptions. Rust does not \`unwrap()\` Android/JNI input.

## Command ABI

Commands are JSON passed to \`NativeBridge.sendCommand(handle, json)\`. Current forms are:

~~~json
{"type":"start"}
{"type":"set_phone_number","phone_number":"+10000000000"}
{"type":"submit_code","code":"12345"}
{"type":"submit_password","password":"..."}
{"type":"load_chats","limit":50}
{"type":"send_text","chat_id":123,"text":"hello"}
{"type":"shutdown"}
~~~

The bridge converts them into the existing Rust \`CoreCommand\` enum; it does not redefine core models.

## Secrets

Do not add Telegram API ID/hash, auth tokens, keystores, session files, or personal Telegram data to this project.
