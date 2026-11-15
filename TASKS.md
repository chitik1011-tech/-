# Parallel work board

## T0 — bootstrap
Status: active

- [ ] T1 Rust workspace + core crate
- [ ] T2 Android app skeleton + Rust JNI loading
- [ ] T3 Core event/state model
- [ ] T4 Telegram backend abstraction
- [ ] T5 CI for Rust + Android build

## T1 — Rust workspace + core crate
Suggested branch: `agent/core-bootstrap`
Files: `Cargo.toml`, `crates/core/**`

Deliver:
- workspace
- core crate
- error type
- runtime-safe entry points
- basic unit tests

## T2 — Android JNI shell
Suggested branch: `agent/android-jni`
Files: `android/**`

Deliver:
- minimal Android app
- load native Rust library
- one JNI round-trip
- arm64-v8a first

## T3 — event/state model
Suggested branch: `agent/core-state`
Files: `crates/core/src/model/**`, `crates/core/src/event/**`

Deliver:
- AccountState
- ChatSummary
- MessageSummary
- CoreEvent
- commands/events boundary

## T4 — Telegram backend abstraction
Suggested branch: `agent/protocol-backend`
Files: `crates/telegram/**`

Deliver:
- trait/API boundary independent of UI
- authorization states
- update stream abstraction
- no credentials committed

## T5 — CI/build
Suggested branch: `agent/build-ci`
Files: `.github/workflows/**`, build docs

Deliver:
- cargo fmt/check/test
- Android debug build once Android shell lands
- cache dependencies

## Integration order

T1 first. T3/T4 can follow immediately. T2 can proceed in parallel after the native library name/interface is agreed. T5 tracks all of them.
