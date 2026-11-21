# Fuu (封)

[![License: GPL-3.0](https://img.shields.io/github/license/palf-gh/Fuu)](LICENSE)
[![Release](https://img.shields.io/github/v/release/palf-gh/Fuu?include_prereleases&label=release)](https://github.com/palf-gh/Fuu/releases)
[![Downloads](https://img.shields.io/github/downloads/palf-gh/Fuu/total)](https://github.com/palf-gh/Fuu/releases)
[![Rust 1.95+](https://img.shields.io/badge/Rust-1.95%2B-dea584?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)](https://github.com/palf-gh/Fuu/releases)

[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-Hans.md) | [繁體中文](README.zh-Hant.md) | **한국어** | [Português (Brasil)](README.pt-BR.md) | [Español (España)](README.es-ES.md) | [Español (Latinoamérica)](README.es-419.md) | [Deutsch](README.de.md) | [Français](README.fr.md) | [Bahasa Indonesia](README.id.md)

> 이 README는 [일본어 원문](README.ja.md)을 AI로 번역한 것입니다. 수정 제안을 환영합니다.

Fuu는 Windows, macOS, Linux에서 쓸 수 있는, 기능을 줄인 메일 클라이언트입니다. 메일을 읽고, 쓰고, 답장하는 일에 집중합니다. 여러 계정을 쓸 수 있고, 계정마다 서명을 따로 설정할 수 있습니다. Fuu는 메일 서버에 IMAP / SMTP로 직접 연결하며, 중간에 다른 서버를 거치지 않습니다. 이름은 한자 「封」의 뜻(봉하다, 닫아서 지키다)에서 따왔습니다.

> **알파 버전입니다.** 기본 기능은 갖추었지만, 버그가 있거나 동작이 바뀔 수 있습니다.

## 다운로드

[Releases](https://github.com/palf-gh/Fuu/releases)에서 사용 중인 OS에 맞는 파일을 고르세요.

| OS | 파일 |
|---|---|
| Windows 10 / 11 (x64) | `fuu_<버전>_x64-setup.exe` (관리자 권한 필요 없음) |
| macOS 11 이상 (Apple 실리콘 / Intel) | `Fuu_<버전>_aarch64.dmg` / `Fuu_<버전>_x86_64.dmg` |
| Linux (x86_64) | `fuu_<버전>_amd64.deb` / `fuu_<버전>_x86_64.AppImage` |

- macOS 버전은 아직 Apple의 서명과 공증을 받지 않았습니다. 처음 열 때는 시스템 설정 → 개인정보 보호 및 보안에서 「확인 없이 열기」를 선택하세요.
- Windows 설치 프로그램에도 서명이 없어서 SmartScreen 경고가 나타날 수 있습니다.
- Linux에서 비밀번호를 저장하려면 GNOME 키링이나 KWallet 같은 Secret Service가 필요합니다.

## 할 수 있는 일

- 메일 읽기, 새로 쓰기, 답장, 전체 답장, 전달
- 여러 계정. 로그인은 비밀번호(앱 비밀번호) 또는 Google · Microsoft의 OAuth 2.0
- 계정별 서명
- HTML 메일은 위험한 부분을 제거한 뒤에 표시합니다. 외부 이미지는 허용했을 때만 불러옵니다
- 첨부 파일 미리 보기(이미지, PDF, Word, Excel, PowerPoint)
- 굵게, 목록 등을 쓸 수 있는 작성 창
- 받은편지함의 새 메일을 자동으로 반영(IMAP IDLE)
- 목록을 날짜별로 묶고, 목록에서 바로 보관 · 삭제. 휴지통에서 받은편지함으로 되돌릴 수도 있습니다
- 메일 주소 링크(`mailto:`)를 열고, 기본 메일 앱으로 설정할 수도 있습니다
- 새 버전을 자동으로 확인하고, 서명을 확인한 뒤 업데이트

## 언어

화면은 영어, 일본어, 중국어(간체 · 번체), 한국어, 포르투갈어(브라질), 스페인어(스페인 · 라틴 아메리카), 독일어, 프랑스어, 인도네시아어를 지원합니다. OS 언어에 맞춰 선택되며, 설정 → 언어에서 바꿀 수도 있습니다.

일본어 이외의 번역과 각 언어의 README는 AI로 번역했습니다. 어색하거나 틀린 곳을 발견하면 Issue나 Pull Request로 알려 주세요. 번역 파일은 [`lang/`](lang)에 있습니다.

## 키보드 단축키

| 키 | 동작 |
|---|---|
| `j` / `k` (또는 ↓ / ↑) | 다음 / 이전 메일 |
| `r` | 답장 |
| `a` | 전체 답장 |
| `f` | 전달 |
| `e` | 보관(휴지통과 스팸에서는 받은편지함으로 되돌리기) |
| `Delete` / `#` | 삭제 |
| `c` | 새 메일 |
| `F5` | 표시 중인 폴더 다시 불러오기 |
| `Ctrl+Enter` (macOS는 `Cmd+Enter`) | 보내기 |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | 탭 열기 / 닫기 / 닫은 탭 다시 열기 |
| `Ctrl+,` | 설정 |

한 글자 키는 입력란을 선택하지 않았을 때만 작동합니다.

## 기본 메일 앱으로 설정하기

설정 → 일반에서 「Fuu를 기본 앱으로 설정」을 누르면 메일 주소 링크를 Fuu로 열 수 있습니다.

- **Windows**: Fuu를 메일 앱으로 등록한 뒤 「기본 앱」 화면이 열립니다. 거기서 Fuu를 선택하세요. Windows에서는 앱이 스스로 기본 앱이 될 수 없습니다.
- **macOS**: `Fuu.app`에서 실행했을 때 설정할 수 있습니다.
- **Linux**: Fuu의 desktop 파일을 `~/.local/share/applications`에 두고 `xdg-mime`으로 기본값으로 설정합니다.

링크에서 가져오는 것은 받는 사람, 참조, 숨은 참조, 제목, 본문뿐이며, 첨부 파일 지정은 무시합니다. 보내기는 항상 직접 합니다.

## 업데이트

Fuu는 실행하고 잠시 뒤 GitHub에서 새 버전이 있는지 확인합니다. 새 버전이 있으면 제목 표시줄에 「업데이트」 버튼이 나타납니다. 누르면 업데이트 파일을 내려받고, 서명을 확인한 뒤 교체합니다. 설정 → 일반에서 이 확인을 끄거나 바로 확인할 수 있습니다. `.deb`로 설치한 경우에는 Releases 페이지를 엽니다.

확인할 때 보내는 것은 GitHub로의 HTTPS 요청 한 번뿐입니다. 사용 정보 등은 보내지 않습니다.

## OAuth로 로그인(Gmail, Outlook.com)

Fuu에는 OAuth 클라이언트 ID가 들어 있지 않습니다. 직접 등록해서 `oauth.toml`에 적어 주세요. 위치는 `accounts.toml`과 같은 폴더입니다(Windows: `%APPDATA%\Fuu\oauth.toml`, macOS: `~/Library/Application Support/Fuu/oauth.toml`, Linux: `~/.config/Fuu/oauth.toml`).

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # 데스크톱 클라이언트에도 발급되지만, 비밀 정보는 아닙니다

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

환경 변수 `FUU_GOOGLE_CLIENT_ID`, `FUU_GOOGLE_CLIENT_SECRET`, `FUU_MICROSOFT_CLIENT_ID`가 있으면 파일보다 우선합니다. 파일을 고친 뒤에는 Fuu를 다시 시작하세요.

**Google** (Google Cloud 콘솔)

1. 프로젝트를 만들고 Gmail API를 사용 설정한 뒤 OAuth 동의 화면을 설정합니다(유형은 「External」. 자신을 테스트 사용자로 추가합니다).
2. 범위 `https://mail.google.com/`을 추가합니다.
3. 「Credentials」→「OAuth client ID」에서 「Desktop app」을 만듭니다. 리디렉션 URI는 등록하지 않아도 됩니다(Fuu는 `http://127.0.0.1:<임의의 포트>/`를 씁니다).
4. 앱이 「Testing」 상태인 동안에는 7일마다 다시 로그인하라는 요청을 받습니다. 이것은 Google의 규칙이며, Fuu의 버그가 아닙니다.

**Microsoft** (Microsoft Entra 관리 센터 →「App registrations」)

1. 새로 등록하고, 지원되는 계정 유형을 「모든 조직 디렉터리의 계정 및 개인 Microsoft 계정」으로 합니다.
2. 「Authentication」→「Add a platform」→「Mobile and desktop applications」에서 리디렉션 URI에 `http://localhost`를 추가합니다. 「Allow public client flows」를 켭니다. 클라이언트 암호는 필요 없습니다.
3. 「API permissions」에서 *Office 365 Exchange Online*의 위임된 권한 `IMAP.AccessAsUser.All`과 `SMTP.Send`(그리고 `offline_access`)를 추가합니다.

로그인은 평소 쓰는 브라우저에서 합니다. 새로 고침 토큰은 OS 키체인에만 저장합니다.

## 개발자용

빌드, 테스트, 패키지 만들기, 릴리스, 번역 절차는 [docs/development.md](docs/development.md)(영어)에 정리되어 있습니다. 설계는 [ARCHITECTURE.md](ARCHITECTURE.md), 보안을 위한 구조는 [SECURITY.md](SECURITY.md)를 보세요.

## 라이선스

GPL-3.0-only. Fuu는 Slint를 GPLv3에 따라 사용합니다.
