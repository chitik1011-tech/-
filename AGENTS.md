# Agent collaboration rules

This repository is worked on by multiple coding agents, including mobile/Termux agents.

## Golden rules

1. Never push feature work directly to `main`.
2. One agent = one branch = one scoped task.
3. Branch names:
   - `agent/core-<topic>`
   - `agent/android-<topic>`
   - `agent/protocol-<topic>`
   - `agent/build-<topic>`
4. Before editing:
   ```bash
   git fetch origin
   git checkout main
   git pull --ff-only
   git checkout -b agent/<scope>-<topic>
   ```
5. Keep commits small and descriptive.
6. Open a PR when the assigned slice builds or has tests.
7. Do not rewrite unrelated code.
8. Do not commit API IDs, API hashes, auth tokens, session files, signing keys, keystores or personal Telegram data.

## Architecture ownership

### Rust core
Owns:
- application state
- async runtime
- Telegram client abstraction
- update/event processing
- chat/message models
- persistence interfaces
- networking/protocol integration
- JNI-facing stable API

### Android shell
Owns:
- Activity / Compose UI
- Android lifecycle
- permissions
- notifications
- foreground/background services
- JNI loading and marshaling only

Business logic must stay in Rust whenever practical.

## Coordination

Read `TASKS.md` before starting. Claim one unclaimed task by adding your branch name and status in your PR description. Do not edit another active agent's files unless necessary; if necessary, make the dependency explicit.

## Definition of done

A task is done when:
- code builds for its target,
- public interfaces have concise docs,
- obvious failure paths return errors instead of panicking,
- no secrets are committed,
- the PR explains what changed and how to test it.
