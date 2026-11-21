# Fuu (封)

[![License: GPL-3.0](https://img.shields.io/github/license/palf-gh/Fuu)](LICENSE)
[![Release](https://img.shields.io/github/v/release/palf-gh/Fuu?include_prereleases&label=release)](https://github.com/palf-gh/Fuu/releases)
[![Downloads](https://img.shields.io/github/downloads/palf-gh/Fuu/total)](https://github.com/palf-gh/Fuu/releases)
[![Rust 1.95+](https://img.shields.io/badge/Rust-1.95%2B-dea584?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)](https://github.com/palf-gh/Fuu/releases)

[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-Hans.md) | **繁體中文** | [한국어](README.ko.md) | [Português (Brasil)](README.pt-BR.md) | [Español (España)](README.es-ES.md) | [Español (Latinoamérica)](README.es-419.md) | [Deutsch](README.de.md) | [Français](README.fr.md) | [Bahasa Indonesia](README.id.md)

> 本 README 由 AI 根據[日文原文](README.ja.md)翻譯而成。歡迎指正。

Fuu 是一款功能精簡的郵件用戶端，可在 Windows、macOS 和 Linux 上使用。它專注於閱讀、撰寫和回覆郵件。你可以使用多個帳號，並為每個帳號設定各自的簽名。Fuu 透過 IMAP / SMTP 直接連線到你的郵件伺服器，中間不經過其他伺服器。名稱取自漢字「封」的含義：封存、關閉並守護。

> **這是 Alpha 版。** 基本功能已經齊備，但可能有錯誤，行為也可能改變。

## 下載

請從 [Releases](https://github.com/palf-gh/Fuu/releases) 選擇適合你系統的檔案。

| 系統 | 檔案 |
|---|---|
| Windows 10 / 11（x64） | `fuu_<版本>_x64-setup.exe`（不需要系統管理員權限） |
| macOS 11 或更新版本（Apple 晶片 / Intel） | `Fuu_<版本>_aarch64.dmg` / `Fuu_<版本>_x86_64.dmg` |
| Linux（x86_64） | `fuu_<版本>_amd64.deb` / `fuu_<版本>_x86_64.AppImage` |

- macOS 版尚未經過 Apple 的簽署與公證。第一次開啟時，請在「系統設定」→「隱私權與安全性」中選擇「強制打開」。
- Windows 安裝程式也沒有簽署，因此 SmartScreen 可能會顯示警告。
- 在 Linux 上儲存密碼需要 Secret Service，例如 GNOME 鑰匙圈或 KWallet。

## 功能

- 閱讀、新增、回覆、全部回覆和轉寄郵件
- 多個帳號，可使用密碼（應用程式密碼）或 Google / Microsoft 的 OAuth 2.0 登入
- 每個帳號各自的簽名
- HTML 郵件會先移除危險的部分再顯示；外部圖片只有在你允許時才會載入
- 附件預覽（圖片、PDF、Word、Excel、PowerPoint）
- 可使用粗體和清單等格式的撰寫視窗
- 收件匣的新郵件自動顯示（IMAP IDLE）
- 清單依日期分組；可直接在清單中封存和刪除，也可以把郵件從垃圾桶移回收件匣
- 開啟郵件地址連結（`mailto:`），也可以設為預設郵件 App
- 自動檢查新版本，並在驗證簽名後更新

## 語言

介面支援英文、日文、中文（簡體、繁體）、韓文、葡萄牙文（巴西）、西班牙文（西班牙、拉丁美洲）、德文、法文和印尼文。預設依照系統語言，也可以在「設定」→「語言」中切換。

日文以外的譯文以及各語言的 README 都由 AI 翻譯。如果你發現不自然或錯誤的地方，歡迎透過 Issue 或 Pull Request 告訴我們。翻譯檔案位於 [`lang/`](lang)。

## 鍵盤快速鍵

| 按鍵 | 操作 |
|---|---|
| `j` / `k`（或 ↓ / ↑） | 下一封 / 上一封郵件 |
| `r` | 回覆 |
| `a` | 全部回覆 |
| `f` | 轉寄 |
| `e` | 封存（在垃圾桶和垃圾郵件中：移回收件匣） |
| `Delete` / `#` | 刪除 |
| `c` | 新增郵件 |
| `F5` | 重新載入目前的資料夾 |
| `Ctrl+Enter`（macOS 為 `Cmd+Enter`） | 傳送 |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | 開啟分頁 / 關閉 / 重新開啟已關閉的分頁 |
| `Ctrl+,` | 設定 |

單一字母的按鍵只在未選取輸入欄位時有效。

## 設為預設郵件 App

在「設定」→「一般」中按下「將 Fuu 設為預設」，郵件地址連結就會以 Fuu 開啟。

- **Windows**：Fuu 先將自己登錄為郵件 App，接著開啟「預設應用程式」頁面。請在那裡選擇 Fuu。Windows 不允許 App 自行設為預設。
- **macOS**：從 `Fuu.app` 啟動時可以設定。
- **Linux**：Fuu 將它的 desktop 檔案放到 `~/.local/share/applications`，並用 `xdg-mime` 設為預設。

連結只會填入收件者、副本、密件副本、主旨和內文；指定附件的要求會被忽略。傳送一律由你自己操作。

## 更新

Fuu 啟動後不久會到 GitHub 檢查是否有新版本。如果有，標題列會出現「更新」按鈕。按下後，Fuu 會下載更新、驗證簽名，再換成新版本。你可以在「設定」→「一般」中關閉此檢查，或立即檢查。如果你是用 `.deb` 安裝的，Fuu 會改為開啟 Releases 頁面。

檢查時只會向 GitHub 送出一次 HTTPS 要求，不會傳送任何使用資料。

## 使用 OAuth 登入（Gmail、Outlook.com）

Fuu 不附帶 OAuth 用戶端 ID。請自行註冊，並寫入 `oauth.toml`。這個檔案放在與 `accounts.toml` 相同的資料夾（Windows：`%APPDATA%\Fuu\oauth.toml`；macOS：`~/Library/Application Support/Fuu/oauth.toml`；Linux：`~/.config/Fuu/oauth.toml`）。

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # 桌面用戶端也會取得此值，但它不是機密資訊

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

如果設定了環境變數 `FUU_GOOGLE_CLIENT_ID`、`FUU_GOOGLE_CLIENT_SECRET` 和 `FUU_MICROSOFT_CLIENT_ID`，它們會優先於檔案。修改檔案後請重新啟動 Fuu。

**Google**（Google Cloud 控制台）

1. 建立專案，啟用 Gmail API，並設定 OAuth 同意畫面（類型選「External」；把自己加為測試使用者）。
2. 加入範圍 `https://mail.google.com/`。
3. 在「Credentials」→「OAuth client ID」中建立「Desktop app」。不需要註冊重新導向 URI（Fuu 使用 `http://127.0.0.1:<任意連接埠>/`）。
4. App 處於「Testing」狀態期間，每 7 天會要求重新登入。這是 Google 的規定，不是 Fuu 的錯誤。

**Microsoft**（Microsoft Entra 系統管理中心 →「App registrations」）

1. 註冊新的 App，支援的帳戶類型選擇「任何組織目錄中的帳戶及個人 Microsoft 帳戶」。
2. 在「Authentication」→「Add a platform」→「Mobile and desktop applications」中，加入重新導向 URI `http://localhost`。開啟「Allow public client flows」。不需要用戶端密碼。
3. 在「API permissions」中，加入 *Office 365 Exchange Online* 的委派權限 `IMAP.AccessAsUser.All` 和 `SMTP.Send`（以及 `offline_access`）。

登入會在你平常使用的瀏覽器中進行。重新整理權杖只儲存在系統鑰匙圈中。

## 開發者資訊

建置、測試、封裝、發佈和翻譯的步驟請見 [docs/development.md](docs/development.md)（英文）。設計請見 [ARCHITECTURE.md](ARCHITECTURE.md)，安全機制請見 [SECURITY.md](SECURITY.md)。

## 授權

GPL-3.0-only。Fuu 依 GPLv3 使用 Slint。
