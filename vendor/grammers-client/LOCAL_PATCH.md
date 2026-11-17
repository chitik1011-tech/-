# Local patch provenance

This directory contains the exact published `grammers-client` 0.12.0 crate
source, with one additive auth API patch in `src/client/auth.rs`.

- Upstream project: https://codeberg.org/Lonami/grammers
- Upstream VCS commit recorded by the crate: `e61cf8585e1e9969ab81eda9fb4725a1500e2d86`
- Published crate archive: `grammers-client-0.12.0.crate`
- SHA-256 of the archive: `0d72e6a294d409c22c860d442bc78894507c0ad066733acc27c5d23fed8ef9bb`
- License: MIT OR Apache-2.0; see `LICENSE-MIT` and `LICENSE-APACHE`.

The patch adds `Client::request_login_code_with_delivery_type`, which returns
the existing `LoginToken` and the exact Telegram `SentCodeType` from the same
`auth.sendCode` response. The original `request_login_code` remains available
and delegates to the new method. Its 303 data-center migration, sender
disconnect, and session home-DC update logic are retained in the same code
path.
