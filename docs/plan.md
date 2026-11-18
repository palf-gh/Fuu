# Fuu — ミニマルなメーラー 実装プラン

## Context
`D:\develop\Fuu` に新規リポジトリを作り、機能を絞ったデスクトップメーラーを作る。
- 目的：メールの閲覧・送信・返信（＋署名）
- UI：ミニマリスト・モダン ／ 対応OS：Windows・macOS
- 複数アカウント対応、IMAP/SMTP でメールサーバーに直接接続（中継サーバーなし）
- 決定事項：**Slint + Rust**、認証は**パスワード＋OAuth2**（Gmail / Microsoft）、**軽量キャッシュ**（SQLite）、HTML メールは **wry ウェブビュー併用**、ライセンスは **Fuu・Slint とも GPLv3**
- 設計の柱：**メールの解析エンジンと描画エンジンを完全に分離**し、どちらも後から差し替えられるようにする。**HTML メールのセキュリティ**は多層防御で設計する

## 技術構成
| 役割 | クレート |
|---|---|
| UI | `slint`（winit バックエンド、`raw-window-handle-06` 機能を有効化） |
| HTML 描画（既定の描画エンジン） | `wry`（Win：WebView2 ／ mac：WKWebView、Slint ウィンドウの子 WebView） |
| 予備の描画エンジン | `html2text` → Slint の Text |
| 解析（既定の解析エンジン） | `mail-parser` |
| HTML サニタイズ | `ammonia`（許可リスト方式）＋ `cssparser` による CSS フィルター |
| IMAP | `async-imap` + `tokio-rustls`（TLS 993 / STARTTLS、XOAUTH2） |
| SMTP | `lettre`（`tokio1-rustls-tls`、`Mechanism::Xoauth2`） |
| メール生成 | `mail-builder` |
| OAuth2 | `oauth2`（PKCE + ループバックリダイレクト `http://127.0.0.1:<任意のポート>`） |
| 秘密情報 | `keyring` 3.x（**`windows-native` / `apple-native` 機能が必須**。無いとモックストアに落ちて保存されない） |
| キャッシュ | `rusqlite`（bundled） |
| 外部画像の取得（許可時のみ） | `reqwest`（rustls、Cookie なし） |
| その他 | `tokio`、`dirs`、`serde`/`toml`、`rfd`、`open` |

## エンジン分離の設計
### パイプライン
```
生の RFC 5322 バイト列
  └─▶ [MessageParser トレイト]  解析エンジン（既定：mail-parser 実装）
        └─▶ ParsedMessage        Fuu 独自の中立モデル
              └─▶ [Sanitiser]    セキュリティ層（エンジン非依存・常に通る）
                    └─▶ SafeDocument   無害化済み HTML／テキスト＋リソース一覧＋警告フラグ
                          └─▶ [BodyRenderer トレイト]  描画エンジン（既定：wry、予備：テキスト）
```

### 境界のルール
- **解析エンジンは UI を知らない。描画エンジンは MIME を知らない。** 描画エンジンに渡るのは `SafeDocument` だけで、生のメールや `ParsedMessage` は渡さない
- 境界の型（`ParsedMessage`、`SafeDocument` など）は Fuu 独自の型で定義する。**外部クレートの型を公開 API に出さない**（mail-parser や wry の型が境界をまたがない）
- サニタイザーは描画エンジンの一部にしない。描画エンジンを差し替えても、無害化は同じコードで必ず行われる
- エンジンはファクトリーで選び、設定で切り替えられるようにする（例：WebView が使えない環境では自動的にテキスト描画へ）
- **契約テスト**：同じ `.eml` フィクスチャ群を全ての解析エンジン実装に流し、同じ `ParsedMessage` になることを確認する。描画エンジンも共通のテストスイート（後述のネットワーク要求ゼロ検査など）を全実装で通す

### 主な型とトレイト（概略）
```rust
// fuu-model
pub struct ParsedMessage { headers: Headers, body: Vec<BodyPart>, attachments: Vec<Attachment>, inline: Vec<InlineResource> }
pub enum BodyPart { Text(String), Html(String) }        // 文字コードは全て UTF-8 に正規化済み
pub struct InlineResource { content_id: String, mime: String, data: Bytes }
pub struct SafeDocument { html: String, resources: ResourceManifest, flags: SafetyFlags } // flags: 外部画像あり、リンク偽装の疑い等

// fuu-parser
pub trait MessageParser: Send + Sync { fn parse(&self, raw: &[u8], limits: &ParseLimits) -> Result<ParsedMessage, ParseError>; }

// fuu-render
pub trait BodyRenderer { fn show(&mut self, doc: &SafeDocument, policy: &RemoteContentPolicy); fn set_bounds(&mut self, rect: Rect); fn set_visible(&mut self, v: bool); fn clear(&mut self); }
```

### リポジトリ構成（Cargo ワークスペース、クレートの依存関係で境界を強制）
```
D:\develop\Fuu
  crates/fuu-model/       # 中立モデル（依存は最小限）
  crates/fuu-parser/      # MessageParser トレイト＋mail-parser 実装      → fuu-model
  crates/fuu-sanitiser/   # HTML/CSS 無害化、リンク検査、添付ファイル名検査  → fuu-model
  crates/fuu-core/        # アカウント、IMAP、SMTP、OAuth、キャッシュ、返信生成 → model, parser, sanitiser
  crates/fuu-render/      # BodyRenderer トレイト＋wry 実装＋テキスト実装   → fuu-model のみ
  crates/fuu-app/         # Slint UI、エンジンの組み立て                    → 全て
    ui/*.slint
  tests/fixtures/         # 通常の .eml と悪意のある .eml のコーパス
  fuzz/                   # cargo-fuzz（パーサー、サニタイザー）
  AGENTS.md / CLAUDE.md / LICENSE (GPL-3.0) / README.md / SECURITY.md
```
`fuu-render` は `fuu-parser` に、`fuu-parser` は UI 系に依存できない構成にして、境界違反をコンパイル時に防ぐ。

ブランチ：`main`（リリース）／`develop`（統合）、作業ブランチは `develop` から切る。

## HTML メールのセキュリティ設計
### 表示ポリシー（基本方針）
| 対象 | 方針 | 実装箇所 |
|---|---|---|
| 外部画像 | **既定で読み込み禁止**。このメール1通に限りユーザーが許可したときだけ読み込む | サニタイザーでプレースホルダー化、CSP の `img-src` から `http(s)` を除外、取得は Rust 側のみ（第2〜4層） |
| CID 画像 | **そのメール内の添付データ（`Content-ID` 付きパート）からのみ解決**。ネットワークや他のメール、ローカルファイルは参照しない。見つからない CID は空白にする | `ParsedMessage.inline` → `ResourceManifest` → `<文書オリジン>/<トークン>/res/<連番>` |
| 外部 CSS | **原則として読み込み禁止**。`<link rel=stylesheet>`、`@import`、CSS 内の `url()` は全て削除。例外は設けない | サニタイザー（タグ・CSS フィルター）＋CSP の `style-src` にリモートを含めない |
| リンク | **OS の標準ブラウザで開く**。WebView 内では一切遷移しない。開く前に実際の URL を表示して確認する。`mailto:` は Fuu の新規作成画面を開く | ナビゲーションハンドラーでキャンセル→確認ダイアログ→`open` クレート |
| トラッキングピクセル | **外部画像の制限によって取得を抑止**する（既定では一切取得しない）。さらに、外部画像を許可した場合も、1×1 や非表示（`display:none`、幅・高さ 0〜2px）の外部画像は取得しない | サニタイザーでピクセル候補に印を付け、許可時の取得対象から除外 |
| HTML 内の CSS | **安全性を確認した範囲でのみ適用**。プロパティの許可リスト（色・フォント・文字装飾・余白・枠・幅・揃え・表のレイアウト）だけを残し、他は捨てる | cssparser による `<style>` とインライン `style` のフィルター（第2層） |

### 想定する脅威
| 脅威 | 例 |
|---|---|
| スクリプト実行 | `<script>`、`onerror=`、`javascript:` URL、SVG 内スクリプト、`data:text/html` |
| 追跡・情報漏洩 | 追跡ピクセル、CSS の `url()`・`@import`・`@font-face`、`<link>`、プリフェッチ、Referer |
| フィッシング・偽装 | 表示テキストと実際のリンク先の不一致、IDN ホモグラフ、`position:fixed` で UI を覆う、RTL 制御文字で差出人や添付名を偽装 |
| WebView 内の遷移・入力 | `<form>`、`<meta http-equiv=refresh>`、`<base>`、`<iframe>`/`<object>`/`<embed>`、新規ウィンドウ |
| ローカル資源へのアクセス | `file://`、カスタムプロトコルのパス走査（`cid:../../`） |
| アプリ境界の悪用 | WebView から Rust 側への IPC |
| リソース枯渇 | 巨大メール、深い MIME 入れ子、大量パート、圧縮爆弾的な展開 |
| 添付ファイル | 実行形式の誤起動、ファイル名によるパス走査、Windows の予約名 |

### 第1層：解析時の制限（fuu-parser）
- `ParseLimits`：描画対象の最大サイズ（例 25 MB）、MIME の最大深さ、最大パート数、デコード後の最大サイズ。超えたら描画せず「大きすぎるため表示できません（保存は可能）」と出す
- 解析はワーカーで実行し、タイムアウトと `catch_unwind` でパニックを UI に波及させない
- 文字コードは全て UTF-8 に正規化し、不正なバイト列は置換文字にする

### 第2層：無害化（fuu-sanitiser、許可リスト方式）
- **HTML**：ammonia で許可したタグ・属性だけを残す。段落・見出し・リスト・表・`img`・`a`・`span`/`div`・`b`/`i`/`u`・`blockquote`・`pre` など。`script`、`style` 以外の埋め込み（`iframe`、`object`、`embed`、`form`、`input`、`button`、`link`、`meta`、`base`、`svg`、`math`）と全ての `on*` 属性は削除
- **CSS**：`<style>` 要素とインライン `style` を cssparser で解析し、プロパティの許可リスト（色・フォント・余白・枠・配置の一部）だけを残す。`url()`、`@import`、`@font-face`、`expression()`、`behavior`、`position:fixed/sticky`、極端な `z-index` は削除
- **URL の書き換え**
  - `img`：`cid:` は `<文書オリジン>/<一時トークン>/res/<連番>`（`fuu` カスタムプロトコル。Win は `https://fuu.localhost`、mac は `fuu://localhost`） に書き換える。参照先はリソース一覧の連番だけで、メール中の文字列をパスとして使わない（パス走査を構造的に防ぐ）。`data:` は PNG・JPEG・GIF・WebP の画像だけ許可し、SVG は不可。`http(s):` は既定でプレースホルダーに置き換え、`SafetyFlags::remote_images` を立てる
  - `a href`：`http`・`https`・`mailto` だけ許可し、他は削除。表示テキストがドメインに見えるのに実際のドメインと違う場合や、IDN／Punycode の場合は `SafetyFlags::suspicious_links` に記録
- **テキストメール**：HTML エスケープしてから整形した HTML に包む。自動リンクはしない
- **ヘッダー表示**：差出人名・件名・添付名から双方向制御文字（U+202A〜202E、U+2066〜2069 など）を除く。差出人は表示名と実アドレスを並べて表示

### 第3層：WebView の堅牢化（fuu-render の wry 実装）
- **JavaScript を無効化**：WebView2 は `IsScriptEnabled = false`、WKWebView は `allowsContentJavaScript = false`（wry のプラットフォーム拡張から設定）。サニタイザーと CSP に加えた三重の防御
- **文書はカスタムプロトコルから配信**し、HTTP レスポンスヘッダーで CSP を付ける（meta タグより強い）：
  `default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'; font-src 'none'; script-src 'none'; connect-src 'none'; frame-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'`
- **ネットワークに出さない**：WebView 自身は外部に一切アクセスしない。外部画像を許可したときも、Rust 側が `reqwest` で取得し（Cookie・Referer なし、汎用 UA、サイズ上限、Content-Type が画像であることを確認）、文書と同じオリジン（`res/<連番>`）から配信する
- **遷移の禁止**：ナビゲーションハンドラーで最初の文書以外への遷移を全てキャンセル。`http(s)` のクリックは Slint の確認ダイアログで**実際の遷移先 URL**を表示してから OS の標準ブラウザで開く（`suspicious_links` なら警告を強調）。`mailto:` は Fuu の新規作成画面へ。新規ウィンドウ要求は拒否
- **IPC ハンドラーを登録しない**（WebView から Rust を呼ぶ経路を作らない）。DevTools 無効、コンテキストメニューは最小限、ドラッグ＆ドロップ無効、オートフィル無効
- **状態を残さない**：シークレットモード（`with_incognito`）と一時データディレクトリを使い、メールを切り替えるたびに文書と一時トークンを破棄する。トークンが失効したリソース要求は 404
- `file://` などカスタムプロトコル以外のスキームは全て拒否

### 第4層：外部コンテンツの方針
- 既定は**全てブロック**。閲覧ペインの上部に「外部画像をブロックしました［表示する］」を出す
- 許可はこのメール1通に限る（送信者単位の常に許可は MVP 後）
- 許可した場合も、トラッキングピクセルの候補（1×1、非表示、幅・高さ 0〜2px）は取得しない
- 送信元の開封確認（`Disposition-Notification-To`）には自動で応答しない

### 第5層：添付ファイル
- 自動では開かない。保存はダイアログ経由のみ
- ファイル名を無害化（パス区切り、`..`、制御文字、双方向制御文字、Windows の予約名 `CON`・`NUL` など、末尾のドットや空白）
- 実行形式の拡張子（`.exe .bat .cmd .ps1 .vbs .js .jse .wsf .hta .scr .lnk .msi .jar .app .command .pkg` など）と二重拡張子は警告
- 保存したファイルに Windows では Mark-of-the-Web（`Zone.Identifier` 代替ストリーム）、mac では `com.apple.quarantine` 属性を付け、OS 側の保護を効かせる

### 返信時
- 引用は無害化済みの内容から html2text で作るプレーンテキスト。元の HTML を返信に埋め込まない

### その他の通信・秘密情報
- IMAP/SMTP は TLS 必須（993・465、または STARTTLS を強制）。平文接続は許可しない。証明書検証は無効化できない
- パスワード・トークンは OS キーチェーンにだけ保存し、ログや `accounts.toml` に出さない。OAuth は PKCE と `state` を検証する

## 機能（MVP）
1. **アカウント**：追加・編集・削除。Gmail / Outlook / iCloud のプリセット＋手動入力。接続テストボタン
2. **フォルダー**：IMAP LIST＋SPECIAL-USE（受信・送信済み・下書き・ゴミ箱・アーカイブ）
3. **メール一覧**：新しい順、未読は太字、50件ずつ（ENVELOPE＋FLAGS）、キャッシュから即表示→差分同期
4. **閲覧**：`BODY.PEEK[]` を取得→パイプライン（解析→無害化→描画）。開いたら既読にする。添付一覧と保存
   - wry の子 WebView を閲覧ペインの矩形に重ね、レイアウト変更やリサイズに合わせて `set_bounds`
   - Slint のポップアップやダイアログを開く間は WebView を隠す（子 WebView は Slint の描画より手前に来るため）
5. **作成・返信・全員に返信**：宛先・CC・BCC・件名・本文（プレーンテキスト、UTF-8 で送信）・添付
   - 返信：先頭にカーソル、その下に署名、その下に `> ` 付きの引用（日本語メールの慣習に合わせたトップポスト）。`In-Reply-To`・`References` を設定し、`Re:` の重複を防ぐ
   - 送信後に送信済みフォルダーへ APPEND（Gmail は自動保存されるので `X-GM-EXT-1` かホスト名で判定して省く）。返信元に `\Answered` を付ける
6. **署名**：アカウントごとのプレーンテキスト。`-- ` 区切りで挿入し、ON/OFF を設定できる
7. **基本操作**：既読／未読の切替、削除（ゴミ箱へ MOVE）、新着確認（N 分ごとのポーリング。IDLE は M5）
8. **UI**：3ペイン（サイドバー：アカウント＞フォルダー／一覧／閲覧）。OS のライト・ダークに追従。ショートカット：`j/k` 移動、`r` 返信、`a` 全員に返信、`c` 新規、`Ctrl/Cmd+Enter` 送信。About に GPLv3 表記

**MVP 外**：転送、統合受信箱、全文検索、Thunderbird ISPDB 自動設定、HTML メールの作成、下書きのサーバー保存、送信者単位の画像許可、通知センター連携

## スレッドモデルとデータ保存
- メインスレッド：Slint イベントループ（wry もここで生成。WebView2 の STA 要件と mac のメインスレッド要件を満たす）
- バックグラウンド：tokio マルチスレッドランタイム。アカウントごとに IMAP セッション1本
- UI→core：`tokio::mpsc` のコマンド。core→UI：`slint::invoke_from_event_loop` で `VecModel` を更新
- `dirs::config_dir()/Fuu/accounts.toml`：表示名・アドレス・サーバー設定・署名（秘密情報は書かない）
- `dirs::data_dir()/Fuu/cache.sqlite`：フォルダー、ヘッダー、閲覧済み本文（**生のメールを保存し、表示のたびにパイプラインを通す**。無害化結果はキャッシュしないので、サニタイザーを更新すれば過去のメールにも効く）。UIDVALIDITY が変わったらそのフォルダーのキャッシュを破棄
- OAuth クライアント ID は別のユーザー設定ファイルか環境変数から読み、リポジトリに入れない

## OAuth2 の注意点（README に仕様として明記）
- Google：「デスクトップアプリ」のクライアントを作成。スコープ `https://mail.google.com/` は制限付きで、アプリが「テスト」状態ならテストユーザーは100人まで、**リフレッシュトークンは7日で失効**する（再ログインが必要。バグではない）
- Microsoft：Entra でアプリを登録（個人と組織の両方、パブリッククライアント）。スコープは `https://outlook.office.com/IMAP.AccessAsUser.All`、`https://outlook.office.com/SMTP.Send`、`offline_access`
- フロー：システムブラウザで認可→ループバックで受け取る→キーチェーンに保存→期限が切れたら自動更新

## マイルストーン
- **M0 Slint スパイク（Win・Mac 両方で確認。通らなければスタックを再検討）**
  - 複数行 `TextEdit` での日本語 IME（変換中の文字の表示、候補ウィンドウの位置、確定）
  - CJK グリフの表示とフォールバック（femtovg と Skia を比較して決める）
  - wry の子 WebView の埋め込み：リサイズへの追従、ポップアップとの重なり、DPI。**JavaScript 無効化・カスタムプロトコル・CSP ヘッダー・ナビゲーションのキャンセルが両 OS で効くこと**
  - tokio → `invoke_from_event_loop` → `VecModel` で数千行の `ListView` が滑らかに動くか
- **M1** ワークスペース（6クレート）の雛形、`fuu-model`・トレイト定義、アカウント設定とパスワード認証（keyring の読み書きテスト込み）、フォルダー一覧、メール一覧
- **M2** `fuu-parser`（mail-parser 実装＋制限）、`fuu-sanitiser`（悪意のあるコーパスのテスト込み）、`fuu-render`（wry＋テキスト）、SQLite キャッシュ、既読・削除、添付の保存
- **M3** 作成・送信・返信・全員に返信、署名、送信済みへの APPEND、添付
- **M4** OAuth2（Google・Microsoft）、トークンの自動更新
- **M5** INBOX の IDLE、仕上げ（ショートカット、空状態、エラー表示）、パッケージング（Win：zip/MSI、mac：`.app`。署名・公証は後で）、fuzz の常設

## 作業時の運用
- 最初の編集の前に Concord の `start_work` で claim し、最後に `finish_work`
- 英語の識別子・ドキュメントはイギリス英語の綴り（`sanitiser`、`colour` など）
- 作業後に `D:\develop\.agents\production-record.md` へ1項目追記
- mac のビルドと確認は macOS の実機で行う
- `SECURITY.md` に上記の脅威モデルと防御層を記載し、セキュリティに関わる変更ではそれを更新する

## 検証
- **契約テスト（解析）**：`.eml` フィクスチャ（ISO-2022-JP 本文、Shift_JIS、RFC 2047 件名、multipart/alternative、cid 画像、添付、壊れた MIME）を全ての `MessageParser` 実装に流し、期待する `ParsedMessage` と一致することを確認
- **サニタイザーのテスト**：悪意のあるコーパス（XSS の典型ベクター、`javascript:`／`data:text/html` リンク、SVG スクリプト、CSS の `url()`・`@import`、`meta refresh`、`base`、`form`、`iframe`、追跡ピクセル、`cid:../` パス走査、双方向制御文字、IDN リンク）について、出力に危険要素が残らないこととフラグが立つことを確認
- **表示ポリシーのテスト**：外部画像が既定で取得されないこと、CID がそのメールの添付からだけ解決され未知の CID は空白になること、外部 CSS（`<link>`・`@import`・`url()`）が残らないこと、許可リスト外の CSS プロパティが落ちること、許可時もピクセル候補は取得されないこと
- **描画エンジンの共通テスト**：悪意のあるコーパスを表示させ、WebView から外部への要求がゼロであること、文書オリジン以外のスキーム要求が全て拒否されること、遷移が起きないことを、計数用のカスタムプロトコルで確認
- **fuzz**：`cargo fuzz` でパーサーとサニタイザーを回し、パニックや制限超えがないこと
- **返信生成**：引用・`Re:`・`References`・署名の位置、Gmail で APPEND を省く判定
- **keyring**：Win・Mac 実機で読み書き（モックストアに落ちていないこと）
- **結合テスト**：可能なら Docker の GreenMail か Dovecot で IMAP/SMTP の一連の流れ（一覧→閲覧→送信→送信済みに届く）
- **手動 E2E**：パスワード認証のアカウント（Gmail のアプリパスワードなど）1つと OAuth のアカウント1つで、受信・閲覧・返信・署名・送信済みを確認。日本語で返信を書いて IME も確かめる。実際のニュースレター（HTML）で外部画像がブロックされ、許可すると表示されることを確認
- **ビルド**：Windows で `cargo build --release`、macOS の実機で同じビルドを行い起動を確認
