# Fuu

A minimal desktop mail client for Windows and macOS: read, send and reply, with
multiple accounts and per-account signatures. Fuu talks to your IMAP/SMTP servers
directly; there is no relay service.

Status: **M0 spike** (Slint shell, hardened HTML viewer, model bridge). Mail access
arrives in M1 onwards — see `docs/plan.md`.

## Build

```sh
cargo run -p fuu-app            # debug
cargo build -p fuu-app --release
cargo test --workspace
```

Optional Skia renderer (for comparing CJK rendering against the default femtovg):

```sh
cargo run -p fuu-app --features skia
SLINT_BACKEND=winit-skia cargo run -p fuu-app --features skia
```

Windows needs the WebView2 runtime (bundled with Windows 11). macOS uses WKWebView.

## Layout

| Crate | Role |
|---|---|
| `fuu-model` | Engine-neutral types shared across boundaries |
| `fuu-parser` | `MessageParser` trait + default implementation |
| `fuu-sanitiser` | Allow-list HTML/CSS sanitiser |
| `fuu-core` | Accounts, IMAP/SMTP, OAuth, cache, compose |
| `fuu-render` | `BodyRenderer` trait, hardened webview and text renderers |
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
