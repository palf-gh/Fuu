# Fuu (封)

[日本語](#日本語) | [English](#english)

## 日本語

Windows・macOS 向けの、機能を絞ったミニマルなメールクライアントです。メールの閲覧・
送信・返信ができ、複数のアカウントとアカウントごとの署名に対応しています。Fuu は
IMAP / SMTP サーバーに直接接続し、中継サーバーは使いません。名前は漢字の「封」の字義
（封をする、閉じて守る）から来ています。

現在の状態: **M5** — アカウント（パスワード／アプリ パスワード、または Google /
Microsoft の OAuth 2.0 ログイン）、フォルダー、無害化した本文の表示（Blitz）、
許可したときだけの外部画像、添付ファイル、リッチテキストの作成画面、SMTP での送信・
返信・転送に対応しています。M5 で受信トレイの IDLE、通知とショートカット、ファズテスト、配布用パッケージを
加えました（`docs/plan.md`）。

アカウントは `<設定フォルダー>/Fuu/accounts.toml` に保存します（パスワードなどの
秘密情報は書きません）。`FUU_SPIKE_FEED=1` を付けて起動すると、実際のアカウントの
代わりにダミーデータを表示します。

### ビルド

```sh
cargo run -p fuu-app            # デバッグ
cargo build -p fuu-app --release
cargo test --workspace
```

IMAP の結合テストは、ローカルの GreenMail サーバーに対して実行します（実際の
アカウントは使いません）:

```sh
docker run -d --name fuu-greenmail -p 127.0.0.1:3025:3025 -p 127.0.0.1:3143:3143 \
  -e GREENMAIL_OPTS="-Dgreenmail.setup.test.all -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.users=test:secret@localhost" \
  greenmail/standalone:2.1.3
FUU_GREENMAIL=1 cargo test -p fuu-core --features insecure-test-transport --test greenmail
cargo test -p fuu-core -- --ignored keychain   # 実際の OS キーチェーンでの読み書き
```

配布用パッケージ（`dist/` に出力。署名・公証はまだです）:

```sh
pwsh scripts/package-windows.ps1   # Fuu-<版>-windows-x64.zip
sh scripts/package-macos.sh        # Fuu.app と Fuu-<版>-macos-<arch>.zip（Mac で実行）
```

ファズテスト（nightly と cargo-fuzz が必要。シードは `fuzz/corpus/`）:

```sh
cargo +nightly fuzz run pipeline fuzz/corpus/pipeline   # css / headers も同様
```

同じシードとその決まった変異は、通常の `cargo test` でも毎回流しています
（`crates/fuu-core/tests/fuzz_replay.rs`）。見つかったクラッシュの入力はコーパスに加えてください。

Skia レンダラー（既定の femtovg と CJK の表示を比べる場合）:

```sh
cargo run -p fuu-app --features skia
SLINT_BACKEND=winit-skia cargo run -p fuu-app --features skia
```

### キーボード ショートカット

| キー | 操作 |
|---|---|
| `j` / `k`（または ↓ / ↑） | 次 / 前のメール |
| `r` | 返信 |
| `a` | 全員に返信 |
| `f` | 転送 |
| `e` | アーカイブ |
| `Delete` / `#` | 削除 |
| `c` | 新規作成 |
| `F5` | 表示中のフォルダーを再読み込み |
| `Ctrl+Enter`（macOS は `Cmd+Enter`） | 送信 |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | タブを開く / 閉じる / 閉じたタブを開き直す |
| `Ctrl+,` | 設定 |

1 文字のキーは、入力欄にフォーカスがないときだけ働きます。受信トレイは IDLE で
見張っており、新着メールは自動で一覧に入ります（IDLE がないサーバーでは 5 分ごとに確認します）。

### 構成

| クレート | 役割 |
|---|---|
| `fuu-model` | 境界をまたいで共有する、エンジンに依存しない型 |
| `fuu-parser` | `MessageParser` トレイトと既定の実装 |
| `fuu-sanitiser` | 許可リスト方式の HTML / CSS 無害化 |
| `fuu-core` | アカウント、IMAP / SMTP、OAuth、キャッシュ、メール作成 |
| `fuu-render` | `BodyRenderer` トレイト、Blitz とテキストの描画 |
| `fuu-app` | Slint の UI と全体の組み立て |

詳しくは `ARCHITECTURE.md` と `SECURITY.md` を参照してください。

### OAuth でのログイン（Gmail、Outlook.com）

Fuu には OAuth クライアント ID が含まれていません。ご自身で登録し、
`<設定フォルダー>/Fuu/oauth.toml`（`accounts.toml` と同じ場所。Windows では
`%APPDATA%\Fuu\oauth.toml`、macOS では `~/Library/Application Support/Fuu/oauth.toml`）
に書いてください:

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # デスクトップ クライアントにも発行されますが、機密情報ではありません

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

環境変数 `FUU_GOOGLE_CLIENT_ID`、`FUU_GOOGLE_CLIENT_SECRET`、`FUU_MICROSOFT_CLIENT_ID`
はファイルより優先されます。編集したら Fuu を再起動してください。

**Google**（Google Cloud コンソール）:
1. プロジェクトを作成して **Gmail API** を有効にし、OAuth 同意画面を設定します
   （外部。自分をテストユーザーに追加）。
2. スコープ `https://mail.google.com/` を追加します。
3. 認証情報 → OAuth クライアント ID → **デスクトップ アプリ** を作成します。
   リダイレクト URI の登録は不要です（Fuu は `http://127.0.0.1:<任意のポート>/` を使います）。
4. アプリが「テスト」状態の間、リフレッシュ トークンは 7 日で失効し、Fuu が再ログインを
   求めます。これは Google の仕様で、Fuu の不具合ではありません。

**Microsoft**（Microsoft Entra 管理センター → アプリの登録）:
1. 新規登録で、サポートされるアカウントの種類を「任意の組織ディレクトリ内のアカウントと
   個人の Microsoft アカウント」にします。
2. 認証 → プラットフォームを追加 → **モバイル アプリケーションとデスクトップ
   アプリケーション** → カスタム リダイレクト URI に `http://localhost` を追加します
   （localhost ではポートは無視されます）。「パブリック クライアント フローを許可する」を
   有効にします。クライアント シークレットは不要です。
3. API のアクセス許可 → *Office 365 Exchange Online* の委任されたアクセス許可
   `IMAP.AccessAsUser.All` と `SMTP.Send`（および `offline_access`）を追加します。

仕組み: 普段使っているブラウザで PKCE 付きの認可コード フローを行い、Fuu はループバック
ポートでリダイレクトを受け取ります。リフレッシュ トークンは OS のキーチェーンに保存し
（長い場合は複数のエントリーに分割）、アクセス トークンはメモリにだけ保持します。
IMAP / SMTP には SASL XOAUTH2 でログインします。

### ライセンス

GPL-3.0-only。Fuu は Slint を GPLv3 で利用しています。

## English

A minimal desktop mail client for Windows and macOS: read, send and reply, with
multiple accounts and per-account signatures. Fuu talks to your IMAP/SMTP servers
directly; there is no relay service. The name comes from the meaning of the
character 封 (*fū*): to seal, to close something up and keep it safe.

Status: **M5** — accounts (password / app password, or Google / Microsoft sign-in
with OAuth 2.0), folders, sanitised bodies (Blitz), remote images on request,
attachments, a rich-text composer, and sending, replying and forwarding over SMTP.
M5 adds IDLE on INBOX, notices and shortcuts, fuzzing and packages — see `docs/plan.md`.

Accounts are stored in `<config dir>/Fuu/accounts.toml` (no secrets). Run with
`FUU_SPIKE_FEED=1` to use dummy data instead of real accounts.

### Build

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

Packages (written to `dist/`; not signed or notarised yet):

```sh
pwsh scripts/package-windows.ps1   # Fuu-<version>-windows-x64.zip
sh scripts/package-macos.sh        # Fuu.app and Fuu-<version>-macos-<arch>.zip (run on a Mac)
```

Fuzzing (needs nightly and cargo-fuzz; seeds live in `fuzz/corpus/`):

```sh
cargo +nightly fuzz run pipeline fuzz/corpus/pipeline   # likewise css / headers
```

The same seeds, plus fixed mutations of them, run in every `cargo test`
(`crates/fuu-core/tests/fuzz_replay.rs`). Add crash reproducers to the corpus.

Optional Skia renderer (for comparing CJK rendering against the default femtovg):

```sh
cargo run -p fuu-app --features skia
SLINT_BACKEND=winit-skia cargo run -p fuu-app --features skia
```


### Keyboard shortcuts

| Key | Action |
|---|---|
| `j` / `k` (or ↓ / ↑) | Next / previous message |
| `r` | Reply |
| `a` | Reply all |
| `f` | Forward |
| `e` | Archive |
| `Delete` / `#` | Delete |
| `c` | New message |
| `F5` | Reload the folder |
| `Ctrl+Enter` (`Cmd+Enter` on macOS) | Send |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | New tab / close tab / reopen closed tab |
| `Ctrl+,` | Settings |

Single keys work only while no text field has focus. INBOX is watched with IDLE,
so new mail appears on its own (servers without IDLE are checked every 5 minutes).

### Layout

| Crate | Role |
|---|---|
| `fuu-model` | Engine-neutral types shared across boundaries |
| `fuu-parser` | `MessageParser` trait + default implementation |
| `fuu-sanitiser` | Allow-list HTML/CSS sanitiser |
| `fuu-core` | Accounts, IMAP/SMTP, OAuth, cache, compose |
| `fuu-render` | `BodyRenderer` trait, Blitz and text renderers |
| `fuu-app` | Slint UI and wiring |

See `ARCHITECTURE.md` and `SECURITY.md`.

### OAuth sign-in (Gmail, Outlook.com)

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

### Licence

GPL-3.0-only. Fuu uses Slint under the GPLv3.
