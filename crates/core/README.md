# tg-mobile-core

The core crate owns UI-independent application state and the command/event boundary used by Android and the Telegram backend.

## Boundaries

Android should talk to the core through `CoreHandle` and should not implement Telegram business logic itself.

Telegram protocol implementations must implement `TelegramBackend` and translate protocol-specific types into the core models and events.

## Runtime

Create the runtime with:

```rust
let handle = spawn_core(backend);
```

Commands can be sent as Rust values with `CoreHandle::send` or through the JSON bridge with `CoreHandle::send_json`.

State snapshots are available through:

- `CoreHandle::state`
- `CoreHandle::state_json`
- `CoreHandle::subscribe_state`

Raw events are available through `CoreHandle::subscribe`.

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

Incoming `MessageAdded` events are cached per chat and deduplicated by message ID. This prevents an optimistic/send result and the corresponding Telegram update from creating duplicate cached messages.

## Safety

This crate forbids unsafe Rust. FFI-specific unsafe code belongs in a narrow Android or protocol bridge crate, not in core.
