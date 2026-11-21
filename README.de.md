# Fuu (封)

[![License: GPL-3.0](https://img.shields.io/github/license/palf-gh/Fuu)](LICENSE)
[![Release](https://img.shields.io/github/v/release/palf-gh/Fuu?include_prereleases&label=release)](https://github.com/palf-gh/Fuu/releases)
[![Downloads](https://img.shields.io/github/downloads/palf-gh/Fuu/total)](https://github.com/palf-gh/Fuu/releases)
[![Rust 1.95+](https://img.shields.io/badge/Rust-1.95%2B-dea584?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)](https://github.com/palf-gh/Fuu/releases)

[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-Hans.md) | [繁體中文](README.zh-Hant.md) | [한국어](README.ko.md) | [Português (Brasil)](README.pt-BR.md) | [Español (España)](README.es-ES.md) | [Español (Latinoamérica)](README.es-419.md) | **Deutsch** | [Français](README.fr.md) | [Bahasa Indonesia](README.id.md)

> Diese README ist eine KI-Übersetzung des [japanischen Originals](README.ja.md). Korrekturen sind willkommen.

Fuu ist ein schlanker Mail-Client für Windows, macOS und Linux. Er konzentriert sich auf das Lesen, Schreiben und Beantworten von E-Mails. Sie können mehrere Konten nutzen und jedem eine eigene Signatur geben. Fuu verbindet sich per IMAP / SMTP direkt mit Ihrem Mailserver, ohne einen anderen Server dazwischen. Der Name stammt von der Bedeutung des Kanji 封: versiegeln, verschließen und schützen.

> **Dies ist eine Alpha-Version.** Die Grundfunktionen sind vorhanden, aber rechnen Sie mit Fehlern und Änderungen im Verhalten.

## Download

Wählen Sie unter [Releases](https://github.com/palf-gh/Fuu/releases) die Datei für Ihr System.

| System | Datei |
|---|---|
| Windows 10 / 11 (x64) | `fuu_<Version>_x64-setup.exe` (keine Administratorrechte nötig) |
| macOS 11 oder neuer (Apple Silicon / Intel) | `Fuu_<Version>_aarch64.dmg` / `Fuu_<Version>_x86_64.dmg` |
| Linux (x86_64) | `fuu_<Version>_amd64.deb` / `fuu_<Version>_x86_64.AppImage` |

- Die macOS-Version ist noch nicht von Apple signiert oder notarisiert. Wählen Sie beim ersten Öffnen unter Systemeinstellungen → Datenschutz & Sicherheit „Dennoch öffnen“.
- Auch der Windows-Installer ist nicht signiert, daher kann SmartScreen eine Warnung anzeigen.
- Unter Linux braucht das Speichern von Passwörtern einen Secret Service wie GNOME Keyring oder KWallet.

## Was Fuu kann

- Lesen, Verfassen, Antworten, Allen antworten und Weiterleiten
- Mehrere Konten, Anmeldung per Passwort (App-Passwort) oder per OAuth 2.0 von Google / Microsoft
- Eine Signatur pro Konto
- HTML-Mails werden erst angezeigt, nachdem gefährliche Teile entfernt wurden; externe Bilder werden nur geladen, wenn Sie es erlauben
- Vorschau von Anhängen (Bilder, PDF, Word, Excel, PowerPoint)
- Ein Verfassen-Fenster mit Fettdruck, Listen und Ähnlichem
- Neue Mails im Posteingang erscheinen von selbst (IMAP IDLE)
- Die Liste ist nach Datum gruppiert; archivieren und löschen Sie direkt aus der Liste und verschieben Sie Mails aus dem Papierkorb zurück in den Posteingang
- Öffnet E-Mail-Links (`mailto:`) und kann Ihre Standard-Mail-App sein
- Sucht nach neuen Versionen und aktualisiert sich, nachdem die Signatur geprüft wurde

## Sprachen

Fuu ist auf Englisch, Japanisch, Chinesisch (vereinfacht und traditionell), Koreanisch, Portugiesisch (Brasilien), Spanisch (Spanien und Lateinamerika), Deutsch, Französisch und Indonesisch verfügbar. Es folgt der Systemsprache, und Sie können sie unter Einstellungen → Sprache wechseln.

Die Übersetzungen außer Japanisch und die README in jeder Sprache wurden mit KI erstellt. Wenn Ihnen etwas Unnatürliches oder Falsches auffällt, sagen Sie es uns bitte in einem Issue oder Pull Request. Die Übersetzungsdateien liegen in [`lang/`](lang).

## Tastenkürzel

| Taste | Aktion |
|---|---|
| `j` / `k` (oder ↓ / ↑) | Nächste / vorherige Nachricht |
| `r` | Antworten |
| `a` | Allen antworten |
| `f` | Weiterleiten |
| `e` | Archivieren (in Papierkorb und Spam: zurück in den Posteingang) |
| `Delete` / `#` | Löschen |
| `c` | Neue Nachricht |
| `F5` | Angezeigten Ordner neu laden |
| `Ctrl+Enter` (`Cmd+Enter` unter macOS) | Senden |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | Tab öffnen / schließen / geschlossenen Tab wieder öffnen |
| `Ctrl+,` | Einstellungen |

Tasten mit einem einzelnen Buchstaben funktionieren nur, wenn kein Textfeld ausgewählt ist.

## Fuu als Standard-Mail-App festlegen

Klicken Sie unter Einstellungen → Allgemein auf „Fuu als Standard festlegen“, dann öffnen sich E-Mail-Links in Fuu.

- **Windows**: Fuu registriert sich als Mail-App, danach öffnet sich die Seite „Standard-Apps“. Wählen Sie dort Fuu. Windows erlaubt Apps nicht, sich selbst zum Standard zu machen.
- **macOS**: verfügbar, wenn Fuu aus `Fuu.app` gestartet wird.
- **Linux**: Fuu legt seine Desktop-Datei in `~/.local/share/applications` ab und setzt sie mit `xdg-mime` als Standard.

Ein Link füllt nur Empfänger, CC, BCC, Betreff und Text aus; Anfragen, Dateien anzuhängen, werden ignoriert. Gesendet wird immer nur von Ihnen.

## Updates

Kurz nach dem Start prüft Fuu auf GitHub, ob es eine neue Version gibt. Wenn ja, erscheint in der Titelleiste eine Schaltfläche zum Aktualisieren. Ein Klick darauf lädt das Update herunter, prüft seine Signatur und tauscht es aus. Unter Einstellungen → Allgemein können Sie die Prüfung abschalten oder sofort prüfen. Wenn Sie das `.deb` installiert haben, öffnet Fuu stattdessen die Releases-Seite.

Die Prüfung sendet eine einzige HTTPS-Anfrage an GitHub. Es werden keine Nutzungsdaten gesendet.

## Anmeldung mit OAuth (Gmail, Outlook.com)

Fuu wird ohne OAuth-Client-IDs ausgeliefert. Registrieren Sie eigene und tragen Sie sie in `oauth.toml` ein, im selben Ordner wie `accounts.toml` (Windows: `%APPDATA%\Fuu\oauth.toml`; macOS: `~/Library/Application Support/Fuu/oauth.toml`; Linux: `~/.config/Fuu/oauth.toml`).

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # auch Desktop-Clients erhalten eines; es ist nicht geheim

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

Die Umgebungsvariablen `FUU_GOOGLE_CLIENT_ID`, `FUU_GOOGLE_CLIENT_SECRET` und `FUU_MICROSOFT_CLIENT_ID` haben Vorrang vor der Datei. Starten Sie Fuu nach dem Bearbeiten neu.

**Google** (Google Cloud Console)

1. Erstellen Sie ein Projekt, aktivieren Sie die Gmail API und richten Sie den OAuth-Zustimmungsbildschirm ein (Typ „External“; fügen Sie sich selbst als Testnutzer hinzu).
2. Fügen Sie den Bereich `https://mail.google.com/` hinzu.
3. Erstellen Sie unter „Credentials“ → „OAuth client ID“ eine „Desktop app“. Ein Weiterleitungs-URI muss nicht registriert werden (Fuu verwendet `http://127.0.0.1:<beliebiger Port>/`).
4. Solange die App im Status „Testing“ ist, werden Sie alle 7 Tage zur erneuten Anmeldung aufgefordert. Das ist eine Regel von Google, kein Fehler in Fuu.

**Microsoft** (Microsoft Entra Admin Center → „App registrations“)

1. Registrieren Sie eine neue App mit den unterstützten Kontotypen „Konten in einem beliebigen Organisationsverzeichnis und persönliche Microsoft-Konten“.
2. Fügen Sie unter „Authentication“ → „Add a platform“ → „Mobile and desktop applications“ `http://localhost` als Weiterleitungs-URI hinzu. Schalten Sie „Allow public client flows“ ein. Ein Clientgeheimnis ist nicht nötig.
3. Fügen Sie unter „API permissions“ die delegierten Berechtigungen von *Office 365 Exchange Online* `IMAP.AccessAsUser.All` und `SMTP.Send` (sowie `offline_access`) hinzu.

Sie melden sich in Ihrem gewohnten Browser an. Das Aktualisierungstoken wird nur im Schlüsselbund des Systems gespeichert.

## Für Entwickler

Bauen, Testen, Paketieren, Veröffentlichen und Übersetzen sind in [docs/development.md](docs/development.md) (auf Englisch) beschrieben. Das Design finden Sie in [ARCHITECTURE.md](ARCHITECTURE.md), und wie Fuu Sie schützt, in [SECURITY.md](SECURITY.md).

## Lizenz

GPL-3.0-only. Fuu verwendet Slint unter der GPLv3.
