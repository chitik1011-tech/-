# tg-mobile-core

The core crate owns UI-independent application state and the command/event boundary used by Android and the Telegram backend.

## Boundaries

Android should talk to the core through `CoreHandle` and should not implement Telegram business logic itself.

Telegram protocol implementations must implement `TelegramBackend` and translate protocol-specific types into the core models and events.

## Runtime

Create an in-memory runtime with:

```rust
let handle = spawn_core(backend);
```

Use persistent state with:

```rust
let store = JsonFileStateStore::new(state_path);
let handle = spawn_core_with_store(backend, store);
```

Commands can be sent as Rust values with `CoreHandle::send` or through the JSON bridge with `CoreHandle::send_json`.

Graceful shutdown is available through `CoreHandle::shutdown`. `CoreHandle::is_closed` reports when the command side has actually closed.

State snapshots are available through:

- `CoreHandle::state`
- `CoreHandle::state_json`
- `CoreHandle::subscribe_state`

Raw events are available through `CoreHandle::subscribe`.

`CoreCommand::kind` and `CoreEvent::kind` return stable snake-case identifiers matching the JSON wire names. Bridges should use these helpers instead of maintaining exhaustive duplicate mappings. `CoreEvent::is_terminal` identifies terminal events.

## Peer references and resolution

`parse_peer_reference` is a pure parser for a full `@username`, `t.me` profile/message URL,
or a stable `tg://user?id=…` deep link. It extracts message IDs and optional `single`,
`thread`, and `comment` metadata without contacting Telegram. Private `/c/` links retain
their internal channel number as a distinct reference; it is not treated as a resolved chat ID.

`CoreCommand::ResolvePeer` delegates a validated reference to the backend. The Grammers
backend resolves public usernames and username message links through Telegram's actual
`contacts.resolveUsername` response. Numeric user IDs without an access hash and private
`/c/` links without a cached peer return explicit `Unknown` or `Unsupported` outcomes;
nonexistent usernames return `NotFound`. Mock mode reports resolution as unsupported rather
than inventing production identities. These identity results carry no access grant:
authorization and permission checks remain the backend's responsibility.

## Telegram backend updates

`TelegramBackend::handle` is the request/response path for `CoreCommand` values.

Real Telegram clients also receive updates while no command is running. A backend can expose that stream by overriding `TelegramBackend::take_updates` and returning a `BackendUpdateReceiver`.

Create the standard bounded channel with:

```rust
let (update_tx, update_rx) = backend_update_channel();
```

The default capacity is exposed as `DEFAULT_BACKEND_UPDATE_CAPACITY`.

The runtime listens to the command queue and backend update stream concurrently. Each successful backend update is reduced into `CoreState` and broadcast as a `CoreEvent`.

An error sent through the backend update channel is translated into `CoreEvent::Error`. Closing the update channel only disables unsolicited updates; command handling continues.

A backend should transfer ownership of its update receiver only once:

```rust
fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
    self.updates.take()
}
```

This is the intended path for Telegram updates such as incoming messages, chat changes, connection/auth changes, and later typing/media updates.

## State stores

The core exposes three storage implementations:

- `NoopStateStore` — no persistence.
- `MemoryStateStore` — useful for tests and process-local caching.
- `JsonFileStateStore` — persists the complete `CoreState` to a JSON file.

File reads and writes are moved onto Tokio blocking workers so filesystem I/O does not block executor threads. File writes use a temporary file followed by rename.

Runtime persistence is coalesced on a short periodic interval instead of writing the full state after every Telegram update. The default interval is exposed as `PERSISTENCE_FLUSH_INTERVAL_MS`. This keeps bursty message traffic from turning into one filesystem write per update while in-memory state and UI subscriptions remain immediate.

A terminal `Stopped` event, explicit `CoreHandle::shutdown`, or command-channel closure performs a final synchronous flush before the runtime exits. Therefore normal shutdown preserves the newest snapshot even if the periodic flush has not fired yet.

When persisted state is restored, transient runtime fields are normalized: the runtime is marked active and an old error is cleared.

Android owns the choice of storage path. The core does not assume a platform-specific directory.

## JSON wire format

Commands and events use an adjacent tagged representation with:

- `type` — snake_case variant name
- `payload` — variant data when present

Example command:

```json
{
  "type": "send_text",
  "payload": {
    "chat_id": 42,
    "text": "hello"
  }
}
```

A unit command has no payload:

```json
{
  "type": "start"
}
```

This wire format is intended for the Android/JNI bridge. Keep it backward compatible when possible.

## State reduction

`CoreState` is updated only from `CoreEvent` values.

Incoming `MessageAdded` events are cached per chat and deduplicated by message ID. The cache is bounded per chat for mobile memory safety. Newly inserted incoming messages increment unread count once; duplicate or edited updates of the same message do not.

## Safety

This crate forbids unsafe Rust. FFI-specific unsafe code belongs in a narrow Android or protocol bridge crate, not in core.
