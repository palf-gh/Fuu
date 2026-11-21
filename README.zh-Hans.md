# Fuu (封)

[![License: GPL-3.0](https://img.shields.io/github/license/palf-gh/Fuu)](LICENSE)
[![Release](https://img.shields.io/github/v/release/palf-gh/Fuu?include_prereleases&label=release)](https://github.com/palf-gh/Fuu/releases)
[![Downloads](https://img.shields.io/github/downloads/palf-gh/Fuu/total)](https://github.com/palf-gh/Fuu/releases)
[![Rust 1.95+](https://img.shields.io/badge/Rust-1.95%2B-dea584?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)](https://github.com/palf-gh/Fuu/releases)

[English](README.md) | [日本語](README.ja.md) | **简体中文** | [繁體中文](README.zh-Hant.md) | [한국어](README.ko.md) | [Português (Brasil)](README.pt-BR.md) | [Español (España)](README.es-ES.md) | [Español (Latinoamérica)](README.es-419.md) | [Deutsch](README.de.md) | [Français](README.fr.md) | [Bahasa Indonesia](README.id.md)

> 本 README 由 AI 根据[日文原文](README.ja.md)翻译而成。欢迎指正。

Fuu 是一款功能精简的邮件客户端，可在 Windows、macOS 和 Linux 上使用。它专注于阅读、撰写和回复邮件。你可以使用多个账户，并为每个账户设置各自的签名。Fuu 通过 IMAP / SMTP 直接连接你的邮件服务器，中间不经过其他服务器。名字取自汉字「封」的含义：封存、关闭并守护。

> **这是 Alpha 版。** 基本功能已经齐备，但可能存在错误，行为也可能发生变化。

## 下载

请从 [Releases](https://github.com/palf-gh/Fuu/releases) 中选择适合你系统的文件。

| 系统 | 文件 |
|---|---|
| Windows 10 / 11（x64） | `fuu_<版本>_x64-setup.exe`（无需管理员权限） |
| macOS 11 或更高版本（Apple 芯片 / Intel） | `Fuu_<版本>_aarch64.dmg` / `Fuu_<版本>_x86_64.dmg` |
| Linux（x86_64） | `fuu_<版本>_amd64.deb` / `fuu_<版本>_x86_64.AppImage` |

- macOS 版尚未经过 Apple 的签名和公证。首次打开时，请在「系统设置」→「隐私与安全性」中选择「仍要打开」。
- Windows 安装程序也没有签名，因此 SmartScreen 可能会发出警告。
- 在 Linux 上保存密码需要 Secret Service，例如 GNOME 钥匙环或 KWallet。

## 功能

- 阅读、新建、回复、全部回复和转发邮件
- 多个账户，可使用密码（应用专用密码）或 Google / Microsoft 的 OAuth 2.0 登录
- 每个账户单独的签名
- HTML 邮件在移除危险部分后才会显示；外部图片只有在你允许时才会加载
- 附件预览（图片、PDF、Word、Excel、PowerPoint）
- 支持粗体和列表等格式的撰写窗口
- 收件箱的新邮件自动显示（IMAP IDLE）
- 列表按日期分组；可直接在列表中归档和删除，也可以把邮件从废纸篓移回收件箱
- 打开邮件地址链接（`mailto:`），也可以设为默认邮件应用
- 自动检查新版本，并在验证签名后更新

## 语言

界面支持英语、日语、中文（简体、繁体）、韩语、葡萄牙语（巴西）、西班牙语（西班牙、拉丁美洲）、德语、法语和印度尼西亚语。默认跟随系统语言，也可以在「设置」→「语言」中切换。

除日语外的译文以及各语言的 README 均由 AI 翻译。如果你发现不自然或错误之处，欢迎通过 Issue 或 Pull Request 告诉我们。翻译文件位于 [`lang/`](lang)。

## 键盘快捷键

| 按键 | 操作 |
|---|---|
| `j` / `k`（或 ↓ / ↑） | 下一封 / 上一封邮件 |
| `r` | 回复 |
| `a` | 全部回复 |
| `f` | 转发 |
| `e` | 归档（在废纸篓和垃圾邮件中：移回收件箱） |
| `Delete` / `#` | 删除 |
| `c` | 新建邮件 |
| `F5` | 重新载入当前文件夹 |
| `Ctrl+Enter`（macOS 为 `Cmd+Enter`） | 发送 |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | 打开标签页 / 关闭 / 重新打开已关闭的标签页 |
| `Ctrl+,` | 设置 |

单字母按键只在未选中输入框时有效。

## 设为默认邮件应用

在「设置」→「通用」中按下「将 Fuu 设为默认」，邮件地址链接就会用 Fuu 打开。

- **Windows**：Fuu 先把自己注册为邮件应用，然后打开「默认应用」页面。请在那里选择 Fuu。Windows 不允许应用自行设为默认。
- **macOS**：从 `Fuu.app` 启动时可以设置。
- **Linux**：Fuu 把它的 desktop 文件放到 `~/.local/share/applications`，并用 `xdg-mime` 设为默认。

链接只会填入收件人、抄送、密送、主题和正文；指定附件的请求会被忽略。发送始终由你自己操作。

## 更新

Fuu 启动后不久会在 GitHub 上检查是否有新版本。如果有，标题栏会出现「更新」按钮。按下后，Fuu 会下载更新、验证签名，然后替换为新版本。你可以在「设置」→「通用」中关闭此检查，或立即检查。如果你是用 `.deb` 安装的，Fuu 会改为打开 Releases 页面。

检查时只会向 GitHub 发送一次 HTTPS 请求，不会发送任何使用数据。

## 使用 OAuth 登录（Gmail、Outlook.com）

Fuu 不附带 OAuth 客户端 ID。请自行注册，并写入 `oauth.toml`。该文件放在与 `accounts.toml` 相同的文件夹中（Windows：`%APPDATA%\Fuu\oauth.toml`；macOS：`~/Library/Application Support/Fuu/oauth.toml`；Linux：`~/.config/Fuu/oauth.toml`）。

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # 桌面客户端也会获得此值，但它不是机密信息

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

如果设置了环境变量 `FUU_GOOGLE_CLIENT_ID`、`FUU_GOOGLE_CLIENT_SECRET` 和 `FUU_MICROSOFT_CLIENT_ID`，它们优先于文件。修改文件后请重新启动 Fuu。

**Google**（Google Cloud 控制台）

1. 创建项目，启用 Gmail API，并配置 OAuth 同意屏幕（类型选「External」；把自己添加为测试用户）。
2. 添加范围 `https://mail.google.com/`。
3. 在「Credentials」→「OAuth client ID」中创建「Desktop app」。无需注册重定向 URI（Fuu 使用 `http://127.0.0.1:<任意端口>/`）。
4. 应用处于「Testing」状态期间，每 7 天会要求重新登录。这是 Google 的规定，不是 Fuu 的错误。

**Microsoft**（Microsoft Entra 管理中心 →「App registrations」）

1. 注册新应用，受支持的账户类型选择「任何组织目录中的账户和个人 Microsoft 账户」。
2. 在「Authentication」→「Add a platform」→「Mobile and desktop applications」中，添加重定向 URI `http://localhost`。打开「Allow public client flows」。不需要客户端密码。
3. 在「API permissions」中，添加 *Office 365 Exchange Online* 的委托权限 `IMAP.AccessAsUser.All` 和 `SMTP.Send`（以及 `offline_access`）。

登录在你平时使用的浏览器中进行。刷新令牌只保存在系统钥匙串中。

## 开发者信息

构建、测试、打包、发布和翻译的步骤见 [docs/development.md](docs/development.md)（英文）。设计见 [ARCHITECTURE.md](ARCHITECTURE.md)，安全机制见 [SECURITY.md](SECURITY.md)。

## 许可证

GPL-3.0-only。Fuu 在 GPLv3 下使用 Slint。
