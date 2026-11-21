# Developing Fuu

## Layout

| Crate | Role |
|---|---|
| `fuu-model` | Engine-neutral types shared across boundaries |
| `fuu-parser` | `MessageParser` trait + default implementation |
| `fuu-sanitiser` | Allow-list HTML/CSS sanitiser |
| `fuu-core` | Accounts, IMAP/SMTP, OAuth, cache, compose, preferences |
| `fuu-render` | `BodyRenderer` trait, Blitz and text renderers |
| `fuu-preview` | Attachment previews (images, PDF and Office files to HTML) |
| `fuu-i18n` | UI language selection and `tr!` (gettext catalogues compiled in) |
| `fuu-app` | Slint UI, updater, OS integration and wiring |

See [ARCHITECTURE.md](../ARCHITECTURE.md) and [SECURITY.md](../SECURITY.md).
Agents and contributors follow [CLAUDE.md](../CLAUDE.md) (British spelling,
branches, security notes).

## Build and test

```sh
cargo run -p fuu-app            # debug
cargo build -p fuu-app --release
cargo test --workspace
```

`FUU_SPIKE_FEED=1` shows dummy data instead of real accounts.
`FUU_SNAPSHOT=<file.png> FUU_SNAPSHOT_STEP=<step>` renders one scene to a PNG
and exits (steps are listed in `crates/fuu-app/src/main.rs`); `FUU_LANG=<code>`
picks the language for such runs. Dummy-data and snapshot runs never read or
write preferences and never check for updates.

Building on Linux needs a C toolchain and the display development packages
(Ubuntu: `build-essential pkg-config clang libfontconfig1-dev libfreetype-dev
libxkbcommon-dev libwayland-dev libx11-dev libgl1-mesa-dev`). Japanese PDF
previews use `fonts-noto-cjk`; saving passwords needs a Secret Service (GNOME
Keyring or KWallet).

IMAP integration tests run against a local GreenMail server (never a real account):

```sh
docker run -d --name fuu-greenmail -p 127.0.0.1:3025:3025 -p 127.0.0.1:3143:3143 \
  -e GREENMAIL_OPTS="-Dgreenmail.setup.test.all -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.users=test:secret@localhost" \
  greenmail/standalone:2.1.3
FUU_GREENMAIL=1 cargo test -p fuu-core --features insecure-test-transport --test greenmail
cargo test -p fuu-core -- --ignored keychain   # real OS keychain round trip
```

Fuzzing (needs nightly and cargo-fuzz; seeds live in `fuzz/corpus/`):

```sh
cargo +nightly fuzz run pipeline fuzz/corpus/pipeline   # likewise css / headers / preview
```

The same seeds, plus fixed mutations of them, run in every `cargo test`
(`crates/*/tests/fuzz_replay.rs`). Add crash reproducers to the corpus.

Optional Skia renderer (for comparing CJK rendering against the default femtovg):

```sh
cargo run -p fuu-app --features skia
```

CI (`.github/workflows/ci.yml`) runs the tests on Linux, Windows and macOS for
every push to `develop` and `main`.

## Translations

Source strings are English: `@tr("…")` in `crates/fuu-app/ui/*.slint` and
`fuu_i18n::tr!("…")` in Rust. Placeholders are `{}` (next argument) or `{0}`…
(a given one). The catalogues are `lang/<lang>/LC_MESSAGES/fuu-app.po`; Slint
bundles them and `fuu-i18n` compiles the same files for Rust.

```sh
python scripts/i18n.py               # after changing strings: update lang/*.po and lang/fuu-app.pot
python scripts/i18n.py missing de    # untranslated strings of one language, as JSON
python scripts/i18n.py fill de de.json            # set translations from {"English": "…"}
python scripts/i18n_fill_list.py de list.json     # or from a list in source order (placeholders are checked)
```

Japanese is the original wording; the other languages are AI translations and
say so. Corrections are welcome: edit the `.po` file and open a pull request.

The README follows the same idea: `README.ja.md` is the source. Edit it first,
polish it with the yomiyasu skill, then translate it into `README.md`
(English, the main page) and every `README.<lang>.md`.

## Packages

Packages (written to `dist/`; not signed or notarised yet) are built with
cargo-packager. It fetches its tools (NSIS, …) at pinned versions and verifies
them by hash. The configuration is `[package.metadata.packager]` in
`crates/fuu-app/Cargo.toml`.

```sh
cargo install cargo-packager --version 0.11.8 --locked   # once
cd crates/fuu-app
cargo packager --release --formats nsis           # Windows: fuu_<version>_x64-setup.exe
cargo packager --release --formats app,dmg        # macOS: Fuu.app and a .dmg (run on a Mac)
cargo packager --release --formats deb,appimage   # Linux: a .deb and an AppImage (run on Linux)
```

The Windows installer installs per user into `%LOCALAPPDATA%\Fuu` without
administrator rights. Uninstalling keeps account settings, the cache and
keychain entries.

The icon source is `crates/fuu-app/icons/app/fuu.svg`. After changing it, run
`cargo run -p fuu-app --example make_icons` to regenerate the `.ico`, `.icns`
and PNGs.

## Releases and updates

Releases are built by GitHub Actions (`.github/workflows/release.yml`):

1. Set `version` in the workspace `Cargo.toml` (e.g. `0.1.0-alpha.3`) and add
   `docs/release-notes/v<version>.md`.
2. Merge `develop` into `main`, tag `v<version>` on `main` and push the tag.
3. The workflow builds Windows (NSIS), macOS arm64 and x86_64 (app + dmg) and
   Linux (deb, AppImage), signs the update payloads with the updater key, and
   publishes the release with `SHA256SUMS.txt` and `latest.json`.

Fuu's updater (`crates/fuu-app/src/update.rs`) reads
`https://github.com/palf-gh/Fuu/releases/latest/download/latest.json` and
installs a payload only if its minisign signature matches the public key built
into the app. The private key lives only in the repository secrets
`UPDATER_PRIVATE_KEY` / `UPDATER_PRIVATE_KEY_PASSWORD`; never commit it. If the
key is ever replaced, ship a release with the new public key before signing
with the new private key, or existing installs stop updating.
