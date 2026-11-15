# Contributing

Thanks for helping build Rust Telegram Mobile.

## Before starting

1. Read `AGENTS.md` and `TASKS.md`.
2. Check open pull requests to avoid duplicate work.
3. Start from the latest `main`.
4. Keep one feature/fix per branch and PR.

Suggested branch naming:

- `agent/core-<topic>`
- `agent/protocol-<topic>`
- `agent/android-<topic>`
- `agent/build-<topic>`

## Architecture rule

Keep business logic in Rust whenever practical.

- Rust: state, reducers, protocol integration, persistence, commands/events, business rules.
- Kotlin/Compose: UI, Android lifecycle, permissions, notifications, platform integration.

Do not create a second command/event model in Kotlin when the canonical Rust wire API already exists.

## Required checks

For Rust-affecting changes:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
```

For Android/JNI-affecting changes:

```bash
cd android
./gradlew test
./gradlew assembleDebug
```

Verify the APK packages the expected `arm64-v8a` Rust shared library.

## PR expectations

A pull request should explain:

- what changed;
- why it is needed;
- which files/subsystems are touched;
- test/CI results;
- known limitations;
- overlap or dependency on other open PRs.

Do not merge a change that is known to have failing CI caused by that change.

## Secrets

Never commit:

- Telegram API ID/hash;
- auth tokens;
- login codes or 2FA passwords;
- session databases;
- signing keys/keystores;
- personal Telegram data.

Use local/runtime configuration for secrets.
