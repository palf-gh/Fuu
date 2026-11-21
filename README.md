# Fuu (封)

[![License: GPL-3.0](https://img.shields.io/github/license/palf-gh/Fuu)](LICENSE)
[![Release](https://img.shields.io/github/v/release/palf-gh/Fuu?include_prereleases&label=release)](https://github.com/palf-gh/Fuu/releases)
[![Downloads](https://img.shields.io/github/downloads/palf-gh/Fuu/total)](https://github.com/palf-gh/Fuu/releases)
[![Rust 1.95+](https://img.shields.io/badge/Rust-1.95%2B-dea584?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)](https://github.com/palf-gh/Fuu/releases)

**English** | [日本語](README.ja.md) | [简体中文](README.zh-Hans.md) | [繁體中文](README.zh-Hant.md) | [한국어](README.ko.md) | [Português (Brasil)](README.pt-BR.md) | [Español (España)](README.es-ES.md) | [Español (Latinoamérica)](README.es-419.md) | [Deutsch](README.de.md) | [Français](README.fr.md) | [Bahasa Indonesia](README.id.md)

> This README is an AI translation of the [Japanese original](README.ja.md). Corrections are welcome.

Fuu is a pared-down mail client for Windows, macOS and Linux. It concentrates on reading, writing and replying to mail. You can use several accounts and give each its own signature. Fuu talks to your mail server directly over IMAP / SMTP, with no other server in between. The name comes from the meaning of the kanji 封: to seal, to close and protect.

> **This is an alpha.** The basics are in place, but expect bugs and changes in behaviour.

## Download

Pick the file for your system from [Releases](https://github.com/palf-gh/Fuu/releases).

| System | File |
|---|---|
| Windows 10 / 11 (x64) | `fuu_<version>_x64-setup.exe` (no administrator rights needed) |
| macOS 11 or later (Apple silicon / Intel) | `Fuu_<version>_aarch64.dmg` / `Fuu_<version>_x86_64.dmg` |
| Linux (x86_64) | `fuu_<version>_amd64.deb` / `fuu_<version>_x86_64.AppImage` |

- The macOS build is not yet signed or notarised by Apple. The first time you open it, choose “Open Anyway” under System Settings → Privacy & Security.
- The Windows installer is not signed either, so SmartScreen may warn you.
- On Linux, saving passwords needs a Secret Service such as GNOME Keyring or KWallet.

## What it does

- Read, write, reply, reply to all and forward
- Several accounts, signing in with a password (app password) or Google / Microsoft OAuth 2.0
- A signature per account
- HTML mail is shown only after the dangerous parts are removed; remote images load only when you allow them
- Attachment previews (images, PDF, Word, Excel, PowerPoint)
- A compose window with bold, lists and the like
- New mail in the inbox appears by itself (IMAP IDLE)
- The list is grouped by date; archive and delete straight from the list, and move mail back from Trash to the inbox
- Opens email links (`mailto:`) and can be your default mail app
- Checks for new versions and updates itself after verifying the signature

## Languages

Fuu is available in English, Japanese, Chinese (Simplified and Traditional), Korean, Portuguese (Brazil), Spanish (Spain and Latin America), German, French and Indonesian. It follows your system language, and you can switch under Settings → Language.

The translations other than Japanese, and the README in each language, were made with AI. If you notice anything unnatural or wrong, please tell us in an issue or a pull request. The translation files are in [`lang/`](lang).

## Keyboard shortcuts

| Key | Action |
|---|---|
| `j` / `k` (or ↓ / ↑) | Next / previous message |
| `r` | Reply |
| `a` | Reply all |
| `f` | Forward |
| `e` | Archive (in Trash and Junk: move back to the inbox) |
| `Delete` / `#` | Delete |
| `c` | New message |
| `F5` | Reload the folder shown |
| `Ctrl+Enter` (`Cmd+Enter` on macOS) | Send |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | Open a tab / close it / reopen a closed tab |
| `Ctrl+,` | Settings |

Single-letter keys work only when no text field is selected.

## Making Fuu your default mail app

Press “Make Fuu the default” under Settings → General, and email links will open in Fuu.

- **Windows**: Fuu registers itself as a mail app, then the Default apps page opens. Choose Fuu there. Windows does not let an app make itself the default.
- **macOS**: available when Fuu is started from `Fuu.app`.
- **Linux**: Fuu puts its desktop file in `~/.local/share/applications` and sets it as the default with `xdg-mime`.

A link only fills in the recipients, CC, BCC, subject and body; requests to attach files are ignored. Sending is always up to you.

## Updates

Shortly after starting, Fuu checks GitHub for a new version. If there is one, an “Update” button appears in the title bar. Pressing it downloads the update, verifies its signature and swaps it in. Under Settings → General you can turn the check off or check straight away. If you installed the `.deb`, Fuu opens the Releases page instead.

The check sends a single HTTPS request to GitHub. No usage data is sent.

## Signing in with OAuth (Gmail, Outlook.com)

Fuu does not ship with OAuth client IDs. Register your own and put them in `oauth.toml`, in the same folder as `accounts.toml` (Windows: `%APPDATA%\Fuu\oauth.toml`; macOS: `~/Library/Application Support/Fuu/oauth.toml`; Linux: `~/.config/Fuu/oauth.toml`).

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # desktop clients are issued one too; it is not secret

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

The environment variables `FUU_GOOGLE_CLIENT_ID`, `FUU_GOOGLE_CLIENT_SECRET` and `FUU_MICROSOFT_CLIENT_ID` take priority over the file. Restart Fuu after editing it.

**Google** (Google Cloud console)

1. Create a project, enable the Gmail API and set up the OAuth consent screen (type “External”; add yourself as a test user).
2. Add the scope `https://mail.google.com/`.
3. Under Credentials → OAuth client ID, create a “Desktop app”. No redirect URI needs registering (Fuu uses `http://127.0.0.1:<any port>/`).
4. While the app is in “Testing”, you will be asked to sign in again every 7 days. That is Google’s rule, not a bug in Fuu.

**Microsoft** (Microsoft Entra admin centre → App registrations)

1. Register a new app with supported account types “Accounts in any organisational directory and personal Microsoft accounts”.
2. Under Authentication → Add a platform → Mobile and desktop applications, add `http://localhost` as a redirect URI. Turn on “Allow public client flows”. No client secret is needed.
3. Under API permissions, add the delegated *Office 365 Exchange Online* permissions `IMAP.AccessAsUser.All` and `SMTP.Send` (and `offline_access`).

You sign in with your usual browser. The refresh token is kept only in the system keychain.

## For developers

Building, testing, packaging, releasing and translating are described in [docs/development.md](docs/development.md). See [ARCHITECTURE.md](ARCHITECTURE.md) for the design and [SECURITY.md](SECURITY.md) for how Fuu keeps you safe.

## Licence

GPL-3.0-only. Fuu uses Slint under the GPLv3.
