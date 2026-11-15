# Telegram protocol backend

`tg-mobile-telegram` is the real Telegram/MTProto backend used by the Rust-first Android client. It implements `tg_mobile_core::TelegramBackend`; it is not a mock and it does not use the Bot API.

## Backend choice

This crate uses `grammers-client = 0.12.0`, a Rust high-level MTProto client. It also enables the matching `grammers-session = 0.12.0` SQLite storage feature for persistent authorization state.

TDLib was considered first. TDLib is mature, but using it here would add a C++ cross-build, Android NDK/CMake integration, ABI-specific TDLib shared/static libraries, and an FFI boundary inside the Rust core. The Rust-first `grammers` path already provides authorization, dialogs, messages, ordered update streaming with gap handling, and upload/download APIs while avoiding a second native runtime. That makes it the lower-risk foundation for `aarch64-linux-android` in this repository.

The dependency is pinned to the crates.io `0.12.0` release so all agents build the same API surface. `grammers-client` re-exports its matching sender/session/TL crates and this crate uses those re-exports to avoid cross-version type mismatches.

## What works now

- Persistent SQLite-backed Telegram sessions.
- `CoreCommand::Start` -> `AuthorizationChanged(WaitingForPhoneNumber|Ready)`.
- `SetPhoneNumber` -> request Telegram login code -> `WaitingForCode`.
- `SubmitCode` -> `Ready` or `WaitingForPassword`.
- `SubmitPassword` -> `Ready`.
- `LoadChats { limit }` -> real dialog fetch -> `ChatsLoaded(Vec<ChatSummary>)` with stable chat ID, title, unread count and last message.
- `SendText` -> real Telegram send -> `MessageAdded`.
- A continuously running update worker after authorization. New messages are mapped to `MessageAdded`; message edits and deletions are preserved in the backend-native `TelegramUpdate` stream.
- Immediate unsolicited `MessageAdded` delivery through the core backend update channel.
- Send/update deduplication keyed by `(chat_id, message_id)` so an outgoing message is not emitted twice when Telegram later delivers the matching update.
- Peer references learned from dialogs/incoming messages are cached for later sends.
- Graceful shutdown of the update worker and MTProto sender pool.

No API ID, API hash, phone number, login code, 2FA password, session bytes, or encryption key is hard-coded or committed.

## Configuration

The application creates the backend with values supplied at runtime:

```rust,no_run
use tg_mobile_telegram::{GrammersBackend, TelegramConfig};

# async fn example() -> tg_mobile_core::Result<()> {
# let api_id = std::env::var("TG_API_ID")
#     .map_err(|error| tg_mobile_core::CoreError::InvalidInput(error.to_string()))?
#     .parse::<i32>()
#     .map_err(|error| tg_mobile_core::CoreError::InvalidInput(error.to_string()))?;
# let api_hash = std::env::var("TG_API_HASH")
#     .map_err(|error| tg_mobile_core::CoreError::InvalidInput(error.to_string()))?;
let config = TelegramConfig::new(
    api_id,
    api_hash,
    "/data/user/0/<package>/files/telegram.session",
);
let backend = GrammersBackend::connect(config).await?;
# let _ = backend;
# Ok(())
# }
```

Production Android code should pass credentials from its configuration boundary and put `session_path` under app-private storage. `TelegramConfig` redacts `api_hash` from `Debug` output.

## Android ARM64

There is no TDLib `.so` to package and no Telegram-specific CMake step. The MTProto backend itself is Rust; Cargo-managed dependencies (notably the SQLite/libsql session storage) may still compile native support code through the normal Rust/NDK toolchain. Android cross-compilation therefore still needs the NDK LLVM/Clang linker for the `aarch64-linux-android` target.

Requirements:

- Rust 1.88 or newer for this crate. The existing workspace metadata still says 1.82 for core; this package intentionally raises only its own MSRV because current `grammers` uses newer Rust syntax.
- A recent Android NDK with LLVM/Clang.
- Rust target `aarch64-linux-android`.
- Android min API chosen by the Android shell; API 24+ is a practical baseline for a modern client.

Example cross-check from a host with the NDK configured:

```sh
rustup target add aarch64-linux-android
export ANDROID_NDK_HOME=/path/to/android-ndk
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/<host>/bin/aarch64-linux-android24-clang"
cargo check -p tg-mobile-telegram --target aarch64-linux-android
```

If the Android agent uses `cargo-ndk`, the equivalent target/ABI is `arm64-v8a` / `aarch64-linux-android`.

## Update architecture and current core boundary

`grammers` supplies ordered asynchronous updates independently of command/response calls. `GrammersBackend` starts a dedicated Tokio update task and exposes `subscribe_updates()` for immediate backend-native updates:

- new messages;
- edited messages;
- deleted messages;
- connection lifecycle;
- explicit extension variants for chat changes, unread counts, and typing.

The core runtime exposes `TelegramBackend::take_updates()` and polls the returned receiver alongside commands with `tokio::select!`. `GrammersBackend` connects its continuous Telegram update worker directly to that channel, so incoming `MessageAdded` events reach `CoreHandle::subscribe()` immediately without waiting for another command. The richer `subscribe_updates()` stream remains available for protocol events that do not yet have a `CoreEvent` variant.

Edits, deletions, chat-list changes, unread changes, typing, and connection state still need corresponding `CoreEvent` variants before they can cross the core boundary without loss of semantics.

## Known limitations / next core work

- `SendText` needs a Telegram `PeerRef`; normally `LoadChats` populates this cache first. Incoming messages also populate it.
- `Shutdown` disconnects the transport; it does not log the Telegram account out because the current `CoreCommand` has no logout command.
- Media upload/download are supported by `grammers`, but the current `CoreCommand`/`CoreEvent` types do not expose media operations yet.
- The backend-native enum reserves chat/unread/typing update variants, but raw Telegram updates are not guessed into incomplete core models.
- The SQLite session contains Telegram authorization material. Keep it in Android app-private storage and do not upload or commit it. At-rest encryption is not provided by this crate today.

## Verification

Host checks:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
```

The unit tests do not require Telegram servers. They cover auth-state mapping, chat/message field normalization, peer-ID mapping, error mapping, update-to-core routing, configuration redaction, and outgoing/update deduplication.
