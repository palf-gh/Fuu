# Fuu

A minimal desktop mail client for Windows and macOS: read, send and reply, with
multiple accounts and per-account signatures. Fuu talks to your IMAP/SMTP servers
directly; there is no relay service.

Status: **M4** — accounts (password / app password, or Google / Microsoft sign-in
with OAuth 2.0), folders, sanitised bodies (Blitz), remote images on request,
attachments, a rich-text composer, and sending, replying and forwarding over SMTP.
IDLE and packaging arrive in M5 — see `docs/plan.md`.

Accounts are stored in `<config dir>/Fuu/accounts.toml` (no secrets). Run with
`FUU_SPIKE_FEED=1` to use dummy data instead of real accounts.

## Build

```sh
cargo run -p fuu-app            # debug
cargo build -p fuu-app --release
cargo test --workspace
```

IMAP integration tests run against a local GreenMail server (never a real account):

```sh
docker run -d --name fuu-greenmail -p 127.0.0.1:3025:3025 -p 127.0.0.1:3143:3143 \
  -e GREENMAIL_OPTS="-Dgreenmail.setup.test.all -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.users=test:secret@localhost" \
  greenmail/standalone:2.1.3
FUU_GREENMAIL=1 cargo test -p fuu-core --features insecure-test-transport --test greenmail
cargo test -p fuu-core -- --ignored keychain   # real OS keychain round trip
```

Optional Skia renderer (for comparing CJK rendering against the default femtovg):

```sh
cargo run -p fuu-app --features skia
SLINT_BACKEND=winit-skia cargo run -p fuu-app --features skia
```


## Layout

| Crate | Role |
|---|---|
| `fuu-model` | Engine-neutral types shared across boundaries |
| `fuu-parser` | `MessageParser` trait + default implementation |
| `fuu-sanitiser` | Allow-list HTML/CSS sanitiser |
| `fuu-core` | Accounts, IMAP/SMTP, OAuth, cache, compose |
| `fuu-render` | `BodyRenderer` trait, Blitz and text renderers |
| `fuu-app` | Slint UI and wiring |

See `ARCHITECTURE.md` and `SECURITY.md`.

## OAuth sign-in (Gmail, Outlook.com)

Fuu ships without OAuth client IDs: register your own and tell Fuu about them in
`<config dir>/Fuu/oauth.toml` (next to `accounts.toml`; on Windows
`%APPDATA%\Fuu\oauth.toml`, on macOS `~/Library/Application Support/Fuu/oauth.toml`):

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # Desktop clients have one; it is not confidential

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

Environment variables override the file: `FUU_GOOGLE_CLIENT_ID`,
`FUU_GOOGLE_CLIENT_SECRET`, `FUU_MICROSOFT_CLIENT_ID`. Restart Fuu after editing.

**Google** (Google Cloud console):
1. Create a project, enable the **Gmail API**, and configure the OAuth consent
   screen (External; add yourself as a test user).
2. Add the scope `https://mail.google.com/`.
3. Create credentials → OAuth client ID → **Desktop app**. No redirect URI is
   needed: Fuu uses `http://127.0.0.1:<random port>/`.
4. While the app is in "Testing", refresh tokens expire after 7 days and Fuu asks
   you to sign in again. This is Google policy, not a Fuu bug.

**Microsoft** (Microsoft Entra admin centre → App registrations):
1. New registration; supported account types: *accounts in any organisational
   directory and personal Microsoft accounts*.
2. Authentication → add a platform → **Mobile and desktop applications** →
   custom redirect URI `http://localhost` (the port is ignored for localhost).
   Enable *Allow public client flows*. No client secret.
3. API permissions → add *Office 365 Exchange Online* delegated permissions
   `IMAP.AccessAsUser.All` and `SMTP.Send` (plus `offline_access`).

How it works: sign-in uses the authorisation code flow with PKCE in your normal
browser; Fuu receives the redirect on a loopback port, keeps the refresh token in
the OS keychain (split across entries when long) and the access token in memory
only, and logs in to IMAP/SMTP with SASL XOAUTH2.

## Licence

GPL-3.0-only. Fuu uses Slint under the GPLv3.
