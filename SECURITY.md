# Security Policy

## Reporting a vulnerability

Please do not publish credentials, session data, authentication material, private chats, or exploit details in a public issue.

For now, open a minimal GitHub issue that states a security problem exists **without sensitive reproduction data**, and wait for the maintainer to provide a private contact path.

## Sensitive data

The repository must never contain:

- Telegram API credentials;
- auth tokens;
- login codes or 2FA passwords;
- Telegram session databases;
- signing keys or Android keystores;
- personal Telegram exports or message data.

If any secret is accidentally committed, revoke/rotate it immediately. Deleting it from the latest commit is not sufficient because Git history may still contain it.

## Scope

The project is under active development and has not declared a stable production security support window yet.
