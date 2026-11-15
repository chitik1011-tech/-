# Roadmap

This roadmap describes the public product direction. It is not a promise of dates.

## Foundation — largely integrated

- [x] Rust workspace and core runtime
- [x] Android shell + JNI bridge
- [x] real Telegram backend integration
- [x] authorization state flow
- [x] chat/message state
- [x] connection lifecycle state machine
- [x] reconnect/stale-generation protection
- [x] message history pagination
- [x] message search
- [x] reliable/idempotent text sending
- [x] message edit/delete handling
- [x] drafts
- [x] chat folders
- [x] media cache
- [x] deep links

## Messaging MVP — in progress

- [ ] reactions end-to-end
- [ ] pinned messages end-to-end
- [ ] forwarding end-to-end
- [ ] read receipts polish
- [ ] contacts and peer profiles
- [ ] service messages
- [x] polls end-to-end, including Telegram voting, results, and Android UI
- [ ] media send/download UX
- [ ] replies and message actions UI polish
- [ ] robust notifications/background behavior

## Daily-driver quality

- [ ] complete settings/profile experience
- [ ] groups/channels administration flows
- [ ] stickers/GIF UX
- [ ] media viewer
- [ ] voice messages
- [ ] accessibility pass
- [ ] localization
- [ ] battery/network/performance profiling
- [ ] migration and persistence hardening
- [ ] release signing and reproducible release process

## Later / advanced Telegram surface

- [ ] calls
- [ ] stories
- [ ] advanced channel/group tooling
- [ ] additional Telegram-specific features after the core client is stable

## Release gates

A public stable release should not happen until:

1. authentication and session persistence are reliable;
2. reconnect/update synchronization is tested;
3. message send/edit/delete/search/history are stable;
4. Android lifecycle/background behavior is safe;
5. no credentials or personal session data are packaged;
6. release APK provenance and checksums are published.
