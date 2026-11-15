# Rust Telegram Mobile

Experimental Android Telegram client rewrite with a Rust-first architecture.

## Direction

- Rust owns application/core state, networking abstractions, update processing, storage interfaces and Telegram protocol/client integration.
- Android/Kotlin remains a thin platform layer for Activity/UI, permissions, notifications and JNI.
- No Java/Kotlin business logic should be added unless Android requires it.
- Work is split into small branches and pull requests so several agents can work in parallel.

## Initial milestones

1. Bootstrap Rust workspace and Android JNI bridge.
2. Define core state/event model.
3. Add Telegram client transport integration behind a Rust trait.
4. Implement authorization flow.
5. Implement chat list and message synchronization.
6. Build native Android UI on top of the Rust event API.
7. Add media, notifications, background sync and performance work.

See `AGENTS.md` for collaboration rules.
