# Fuu

A minimal desktop mail client for Windows and macOS: read, send and reply, with
multiple accounts and per-account signatures. Fuu talks to your IMAP/SMTP servers
directly; there is no relay service.

Status: **M3** — accounts (password / app password, kept in the OS keychain),
folders, sanitised bodies (Blitz), remote images on request, attachments, and
sending, replying and forwarding over SMTP. OAuth arrives in M4 — see `docs/plan.md`.

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

## OAuth notes

- Google: the `https://mail.google.com/` scope is restricted. While your OAuth
  client is in "testing" status, refresh tokens expire after 7 days and you must
  sign in again. This is Google policy, not a Fuu bug.
- Microsoft: requires `IMAP.AccessAsUser.All`, `SMTP.Send` and `offline_access`.
- Client IDs are read from user configuration and are never committed.

## Licence

GPL-3.0-only. Fuu uses Slint under the GPLv3.
