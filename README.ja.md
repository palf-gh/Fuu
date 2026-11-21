# Fuu (封)

[![License: GPL-3.0](https://img.shields.io/github/license/palf-gh/Fuu)](LICENSE)
[![Release](https://img.shields.io/github/v/release/palf-gh/Fuu?include_prereleases&label=release)](https://github.com/palf-gh/Fuu/releases)
[![Downloads](https://img.shields.io/github/downloads/palf-gh/Fuu/total)](https://github.com/palf-gh/Fuu/releases)
[![Rust 1.95+](https://img.shields.io/badge/Rust-1.95%2B-dea584?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)](https://github.com/palf-gh/Fuu/releases)

[English](README.md) | **日本語** | [简体中文](README.zh-Hans.md) | [繁體中文](README.zh-Hant.md) | [한국어](README.ko.md) | [Português (Brasil)](README.pt-BR.md) | [Español (España)](README.es-ES.md) | [Español (Latinoamérica)](README.es-419.md) | [Deutsch](README.de.md) | [Français](README.fr.md) | [Bahasa Indonesia](README.id.md)

Fuu は Windows・macOS・Linux で使える、機能を絞ったメールクライアントです。メールを読む、書く、返信することに集中しています。複数のアカウントを使え、アカウントごとに署名を設定できます。Fuu はメールサーバーに IMAP / SMTP で直接つながり、間に別のサーバーを挟みません。名前は漢字の「封」の意味（封をする、閉じて守る）から付けました。

> **アルファ版です。** 基本の機能はそろっていますが、不具合があったり、仕様が変わったりすることがあります。

## ダウンロード

[Releases](https://github.com/palf-gh/Fuu/releases) から、使っている OS のファイルを選んでください。

| OS | ファイル |
|---|---|
| Windows 10 / 11（x64） | `fuu_<バージョン>_x64-setup.exe`（管理者権限は不要です） |
| macOS 11 以降（Apple シリコン / Intel） | `Fuu_<バージョン>_aarch64.dmg` / `Fuu_<バージョン>_x86_64.dmg` |
| Linux（x86_64） | `fuu_<バージョン>_amd64.deb` / `fuu_<バージョン>_x86_64.AppImage` |

- macOS 版はまだ Apple の署名・公証を受けていません。初めて開くときは、システム設定の「プライバシーとセキュリティ」で「このまま開く」を選んでください。
- Windows のインストーラーにも署名がないため、SmartScreen の警告が出ることがあります。
- Linux でパスワードを保存するには、GNOME キーリングや KWallet などの Secret Service が必要です。

## できること

- メールの閲覧、新規作成、返信、全員に返信、転送
- 複数のアカウント。ログインはパスワード（アプリ パスワード）か、Google・Microsoft の OAuth 2.0
- アカウントごとの署名
- HTML メールは危険な部分を取り除いてから表示します。外部の画像は許可したときだけ読み込みます
- 添付ファイルのプレビュー（画像、PDF、Word、Excel、PowerPoint）
- 太字やリストを使える作成画面
- 受信トレイの新着を自動で反映（IMAP IDLE）
- 一覧を日付ごとにまとめ、一覧から直接アーカイブ・削除。ゴミ箱から受信トレイにも戻せます
- メールアドレスのリンク（`mailto:`）を開き、既定のメールアプリにもできます
- 新しいバージョンの自動確認と、署名を確かめたうえでの更新

## 言語

表示は英語、日本語、中国語（簡体字・繁体字）、韓国語、ポルトガル語（ブラジル）、スペイン語（スペイン・ラテンアメリカ）、ドイツ語、フランス語、インドネシア語に対応しています。OS の言語に合わせて選ばれ、設定の「言語」で切り替えることもできます。

日本語以外の訳と、各言語の README は AI で翻訳しました。不自然なところや誤りに気づいたら、Issue や Pull Request で教えてください。訳の元は [`lang/`](lang) にあります。

## キーボード ショートカット

| キー | 操作 |
|---|---|
| `j` / `k`（または ↓ / ↑） | 次 / 前のメール |
| `r` | 返信 |
| `a` | 全員に返信 |
| `f` | 転送 |
| `e` | アーカイブ（ゴミ箱と迷惑メールでは受信トレイに戻す） |
| `Delete` / `#` | 削除 |
| `c` | 新規作成 |
| `F5` | 表示中のフォルダーを読み込み直す |
| `Ctrl+Enter`（macOS は `Cmd+Enter`） | 送信 |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | タブを開く / 閉じる / 閉じたタブを開き直す |
| `Ctrl+,` | 設定 |

1 文字のキーは入力欄を選んでいないときだけ使えます。

## 既定のメールアプリにする

設定の「一般」で「Fuu を既定にする」を押すと、メールアドレスのリンクを Fuu で開けるようになります。

- **Windows**: Fuu をメールアプリとして登録したあと、「既定のアプリ」の画面が開きます。そこで Fuu を選んでください。Windows では、アプリが自分で既定になることはできません。
- **macOS**: `Fuu.app` から起動しているときに設定できます。
- **Linux**: Fuu の desktop ファイルを `~/.local/share/applications` に置き、`xdg-mime` で既定にします。

リンクから取り込むのは宛先・CC・BCC・件名・本文だけで、添付ファイルの指定は無視します。送信は必ず自分で行います。

## 更新

Fuu は起動して少しすると、GitHub に新しいバージョンがないかを確かめます。新しいバージョンがあれば、タイトルバーに「更新」のボタンが出ます。押すと更新ファイルをダウンロードし、署名を確かめてから入れ替えます。設定の「一般」で、この確認を止めたり、すぐに確かめたりできます。`.deb` で入れた場合は、Releases のページを開きます。

確認のときに送るのは、GitHub への 1 回の HTTPS リクエストだけです。利用状況などは送りません。

## OAuth でのログイン（Gmail、Outlook.com）

Fuu には OAuth のクライアント ID が入っていません。ご自身で登録し、`oauth.toml` に書いてください。置き場所は `accounts.toml` と同じフォルダーです（Windows は `%APPDATA%\Fuu\oauth.toml`、macOS は `~/Library/Application Support/Fuu/oauth.toml`、Linux は `~/.config/Fuu/oauth.toml`）。

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # デスクトップ用のクライアントにも発行されますが、秘密の情報ではありません

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

環境変数 `FUU_GOOGLE_CLIENT_ID`、`FUU_GOOGLE_CLIENT_SECRET`、`FUU_MICROSOFT_CLIENT_ID` があれば、ファイルより優先します。書き換えたら Fuu を起動し直してください。

**Google**（Google Cloud コンソール）

1. プロジェクトを作って Gmail API を有効にし、OAuth 同意画面を設定します（種類は「外部」。自分をテストユーザーに加えます）。
2. スコープ `https://mail.google.com/` を加えます。
3. 「認証情報」→「OAuth クライアント ID」で「デスクトップ アプリ」を作ります。リダイレクト URI の登録は要りません（Fuu は `http://127.0.0.1:<任意のポート>/` を使います）。
4. アプリが「テスト」の状態のあいだは、7 日ごとに再ログインを求められます。これは Google の決まりで、Fuu の不具合ではありません。

**Microsoft**（Microsoft Entra 管理センター →「アプリの登録」）

1. 新しく登録し、使えるアカウントの種類を「任意の組織ディレクトリ内のアカウントと個人の Microsoft アカウント」にします。
2. 「認証」→「プラットフォームを追加」→「モバイル アプリケーションとデスクトップ アプリケーション」で、リダイレクト URI に `http://localhost` を加えます。「パブリック クライアント フローを許可する」をオンにします。クライアント シークレットは要りません。
3. 「API のアクセス許可」で、*Office 365 Exchange Online* の委任されたアクセス許可 `IMAP.AccessAsUser.All` と `SMTP.Send`（と `offline_access`）を加えます。

ログインはふだん使っているブラウザで行います。リフレッシュ トークンは OS のキーチェーンにだけ保存します。

## 開発者向け

ビルド、テスト、パッケージ作り、リリース、翻訳の手順は [docs/development.md](docs/development.md) にまとめています（英語）。設計は [ARCHITECTURE.md](ARCHITECTURE.md)、安全のための仕組みは [SECURITY.md](SECURITY.md) を見てください。

## ライセンス

GPL-3.0-only。Fuu は Slint を GPLv3 のもとで使っています。
