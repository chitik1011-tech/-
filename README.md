# Rust Telegram Mobile

> An **unofficial** Android Telegram client with a Rust-first architecture.

Rust Telegram Mobile is an experimental Telegram client that keeps application state, Telegram protocol integration, update processing, persistence, and most business logic in Rust while using Kotlin + Jetpack Compose as a thin Android UI/platform layer.

> **Project status:** active development. The repository is not yet offering a stable public APK release. Do not treat development builds as production-ready.

## Why this project

- Rust-first core with a small Android platform layer
- Real Telegram MTProto integration through the Rust backend
- Generation-safe reconnect and stale-update protection
- Reliable/idempotent outgoing message pipeline
- Native Android UI with JNI-backed state/events
- Multi-agent friendly repository structure with CI validation

## Current capabilities

The integrated `main` branch already includes major pieces of a usable Telegram client foundation:

- Telegram authorization/backend integration
- chat and message state in Rust
- message history pagination
- message search
- message edit/delete update handling
- reliable/idempotent text sending
- connection lifecycle state machine and reconnect synchronization
- chat folders and chat actions
- persisted per-chat drafts
- media cache and media dimension normalization
- deep-link routing
- unread badges and Android loading/empty/error states
- generation-safe transient chat activity
- peer reference resolution
- Telegram polls with end-to-end voting and Compose results UI

Some Telegram features are still incomplete or under active development. See [ROADMAP.md](ROADMAP.md).

## Architecture

```text
Telegram / MTProto
       │
       ▼
crates/telegram
       │
       ▼
crates/core   ── state / reducer / persistence / commands / events
       │
       ▼
crates/android-bridge
       │ JNI + canonical JSON wire API
       ▼
Android / Kotlin / Jetpack Compose
```

Rust owns application state and business logic whenever practical. Android owns UI, lifecycle, permissions, notifications, and other platform-specific behavior.

### Workspace

- `crates/core` — application state, commands/events, reducers, runtime and persistence
- `crates/telegram` — Telegram protocol/backend integration
- `crates/android-bridge` — JNI bridge
- `crates/message-entities` — Telegram message entity helpers
- `crates/media-cache` — local bounded media cache
- `android/` — Android/Compose application

## Build

### Requirements

- Rust 1.82+
- JDK 17+
- Android SDK Platform 37
- Android NDK 28.2.13676358
- `cargo-ndk`

### Rust checks

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
```

### Android debug APK

```bash
rustup target add aarch64-linux-android
cd android
./gradlew assembleDebug
```

The Android build compiles the Rust JNI library for `arm64-v8a`. See [android/README.md](android/README.md) for the full setup.

## CI

Pull requests are expected to pass Rust formatting/check/tests and Android JNI validation before merge. Feature work should be isolated to one branch and one PR.

See [AGENTS.md](AGENTS.md) for collaboration rules.

## Contributing

Contributions are welcome while the project is under active development.

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a PR. Please check existing pull requests first so work is not duplicated.

## Security and secrets

Never commit Telegram API credentials, auth tokens, session databases, login codes, 2FA passwords, signing keys, keystores, or personal Telegram data.

For security reporting guidance, see [SECURITY.md](SECURITY.md).

## Releases

Stable public APK releases are not published yet. When release builds start, they should be distributed through GitHub Releases with checksums and release notes rather than from random mirrors.

## License

The Rust workspace is licensed under the MIT License. See [LICENSE](LICENSE).

## Trademark / Telegram notice

This is an independent, unofficial client project. It is not affiliated with, sponsored by, or endorsed by Telegram. Telegram names and trademarks belong to their respective owners.
