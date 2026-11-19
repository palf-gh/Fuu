# Fuu (封)

[日本語](#日本語) | [English](#english)

## 日本語

Windows・macOS 向けの、機能を絞ったミニマルなメールクライアントです。メールの閲覧・
送信・返信ができ、複数のアカウントとアカウントごとの署名に対応しています。Fuu は
IMAP / SMTP サーバーに直接接続し、中継サーバーは使いません。名前は「封」（封筒の封）
から来ています。

現在の状態: **M4** — アカウント（パスワード／アプリ パスワード、または Google /
Microsoft の OAuth 2.0 ログイン）、フォルダー、無害化した本文の表示（Blitz）、
許可したときだけの外部画像、添付ファイル、リッチテキストの作成画面、SMTP での送信・
返信・転送に対応しています。IDLE とパッケージングは M5 の予定です（`docs/plan.md`）。

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

Skia レンダラー（既定の femtovg と CJK の表示を比べる場合）:

```sh
cargo run -p fuu-app --features skia
SLINT_BACKEND=winit-skia cargo run -p fuu-app --features skia
```

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
directly; there is no relay service. The name comes from 封 (*fū*), "seal", as in
sealing an envelope.

Status: **M4** — accounts (password / app password, or Google / Microsoft sign-in
with OAuth 2.0), folders, sanitised bodies (Blitz), remote images on request,
attachments, a rich-text composer, and sending, replying and forwarding over SMTP.
IDLE and packaging arrive in M5 — see `docs/plan.md`.

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

Optional Skia renderer (for comparing CJK rendering against the default femtovg):

```sh
cargo run -p fuu-app --features skia
SLINT_BACKEND=winit-skia cargo run -p fuu-app --features skia
```


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
