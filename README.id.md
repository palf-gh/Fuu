# Fuu (封)

[![License: GPL-3.0](https://img.shields.io/github/license/palf-gh/Fuu)](LICENSE)
[![Release](https://img.shields.io/github/v/release/palf-gh/Fuu?include_prereleases&label=release)](https://github.com/palf-gh/Fuu/releases)
[![Downloads](https://img.shields.io/github/downloads/palf-gh/Fuu/total)](https://github.com/palf-gh/Fuu/releases)
[![Rust 1.95+](https://img.shields.io/badge/Rust-1.95%2B-dea584?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)](https://github.com/palf-gh/Fuu/releases)

[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-Hans.md) | [繁體中文](README.zh-Hant.md) | [한국어](README.ko.md) | [Português (Brasil)](README.pt-BR.md) | [Español (España)](README.es-ES.md) | [Español (Latinoamérica)](README.es-419.md) | [Deutsch](README.de.md) | [Français](README.fr.md) | **Bahasa Indonesia**

> README ini adalah terjemahan AI dari [naskah asli berbahasa Jepang](README.ja.md). Koreksi sangat diterima.

Fuu adalah klien email ringkas untuk Windows, macOS, dan Linux. Fuu berfokus pada membaca, menulis, dan membalas email. Anda dapat memakai beberapa akun dan memberi tanda tangan sendiri untuk tiap akun. Fuu terhubung langsung ke server email Anda melalui IMAP / SMTP, tanpa server lain di tengahnya. Namanya berasal dari arti kanji 封: menyegel, menutup, dan melindungi.

> **Ini versi alfa.** Fitur dasarnya sudah ada, tetapi mungkin masih ada bug dan perubahan perilaku.

## Unduh

Pilih berkas untuk sistem Anda di [Releases](https://github.com/palf-gh/Fuu/releases).

| Sistem | Berkas |
|---|---|
| Windows 10 / 11 (x64) | `fuu_<versi>_x64-setup.exe` (tidak perlu hak administrator) |
| macOS 11 atau lebih baru (Apple silicon / Intel) | `Fuu_<versi>_aarch64.dmg` / `Fuu_<versi>_x86_64.dmg` |
| Linux (x86_64) | `fuu_<versi>_amd64.deb` / `fuu_<versi>_x86_64.AppImage` |

- Versi macOS belum ditandatangani maupun dinotarisasi oleh Apple. Saat pertama kali membukanya, pilih “Buka Saja” (Open Anyway) di Pengaturan Sistem → Privasi & Keamanan.
- Penginstal Windows juga belum ditandatangani, jadi SmartScreen mungkin menampilkan peringatan.
- Di Linux, menyimpan kata sandi memerlukan Secret Service seperti GNOME Keyring atau KWallet.

## Yang bisa dilakukan

- Membaca, menulis, membalas, membalas semua, dan meneruskan
- Beberapa akun, masuk dengan kata sandi (kata sandi aplikasi) atau OAuth 2.0 Google / Microsoft
- Tanda tangan per akun
- Email HTML hanya ditampilkan setelah bagian yang berbahaya dibuang; gambar eksternal hanya dimuat jika Anda mengizinkannya
- Pratinjau lampiran (gambar, PDF, Word, Excel, PowerPoint)
- Jendela penulisan dengan huruf tebal, daftar, dan sejenisnya
- Email baru di kotak masuk muncul dengan sendirinya (IMAP IDLE)
- Daftar dikelompokkan per tanggal; arsipkan dan hapus langsung dari daftar, dan kembalikan email dari Sampah ke kotak masuk
- Membuka tautan email (`mailto:`) dan bisa menjadi aplikasi email bawaan Anda
- Memeriksa versi baru dan memperbarui diri setelah memverifikasi tanda tangannya

## Bahasa

Fuu tersedia dalam bahasa Inggris, Jepang, Tionghoa (Sederhana dan Tradisional), Korea, Portugis (Brasil), Spanyol (Spanyol dan Amerika Latin), Jerman, Prancis, dan Indonesia. Fuu mengikuti bahasa sistem, dan Anda dapat menggantinya di Pengaturan → Bahasa.

Terjemahan selain bahasa Jepang, serta README dalam tiap bahasa, dibuat dengan AI. Jika Anda menemukan sesuatu yang janggal atau salah, beri tahu kami lewat issue atau pull request. Berkas terjemahan ada di [`lang/`](lang).

## Pintasan keyboard

| Tombol | Tindakan |
|---|---|
| `j` / `k` (atau ↓ / ↑) | Pesan berikutnya / sebelumnya |
| `r` | Balas |
| `a` | Balas semua |
| `f` | Teruskan |
| `e` | Arsipkan (di Sampah dan Spam: kembalikan ke kotak masuk) |
| `Delete` / `#` | Hapus |
| `c` | Pesan baru |
| `F5` | Muat ulang folder yang ditampilkan |
| `Ctrl+Enter` (`Cmd+Enter` di macOS) | Kirim |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | Buka tab / tutup / buka lagi tab yang ditutup |
| `Ctrl+,` | Pengaturan |

Tombol satu huruf hanya berfungsi saat tidak ada kolom teks yang dipilih.

## Menjadikan Fuu aplikasi email bawaan

Tekan “Jadikan Fuu bawaan” di Pengaturan → Umum, dan tautan email akan terbuka di Fuu.

- **Windows**: Fuu mendaftarkan dirinya sebagai aplikasi email, lalu halaman Aplikasi default terbuka. Pilih Fuu di sana. Windows tidak mengizinkan aplikasi menjadikan dirinya bawaan sendiri.
- **macOS**: tersedia saat Fuu dijalankan dari `Fuu.app`.
- **Linux**: Fuu menaruh berkas desktop-nya di `~/.local/share/applications` dan menjadikannya bawaan dengan `xdg-mime`.

Tautan hanya mengisi penerima, CC, BCC, subjek, dan isi; permintaan untuk melampirkan berkas diabaikan. Pengiriman selalu Anda lakukan sendiri.

## Pembaruan

Tak lama setelah dijalankan, Fuu memeriksa GitHub untuk versi baru. Jika ada, tombol “Perbarui” muncul di bilah judul. Saat ditekan, Fuu mengunduh pembaruan, memverifikasi tanda tangannya, lalu menggantinya. Di Pengaturan → Umum, Anda dapat mematikan pemeriksaan ini atau memeriksa saat itu juga. Jika Anda memasang `.deb`, Fuu akan membuka halaman Releases sebagai gantinya.

Pemeriksaan ini hanya mengirim satu permintaan HTTPS ke GitHub. Tidak ada data penggunaan yang dikirim.

## Masuk dengan OAuth (Gmail, Outlook.com)

Fuu tidak disertai ID klien OAuth. Daftarkan milik Anda sendiri dan tulis di `oauth.toml`, di folder yang sama dengan `accounts.toml` (Windows: `%APPDATA%\Fuu\oauth.toml`; macOS: `~/Library/Application Support/Fuu/oauth.toml`; Linux: `~/.config/Fuu/oauth.toml`).

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # klien desktop juga mendapatkannya; ini bukan rahasia

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

Variabel lingkungan `FUU_GOOGLE_CLIENT_ID`, `FUU_GOOGLE_CLIENT_SECRET`, dan `FUU_MICROSOFT_CLIENT_ID` lebih diutamakan daripada berkas. Mulai ulang Fuu setelah mengubahnya.

**Google** (konsol Google Cloud)

1. Buat proyek, aktifkan Gmail API, dan siapkan layar izin OAuth (jenis “External”; tambahkan diri Anda sebagai pengguna uji).
2. Tambahkan cakupan `https://mail.google.com/`.
3. Di “Credentials” → “OAuth client ID”, buat “Desktop app”. Tidak perlu mendaftarkan URI pengalihan (Fuu memakai `http://127.0.0.1:<port apa saja>/`).
4. Selama aplikasi berstatus “Testing”, Anda akan diminta masuk lagi setiap 7 hari. Itu aturan Google, bukan bug di Fuu.

**Microsoft** (pusat admin Microsoft Entra → “App registrations”)

1. Daftarkan aplikasi baru dengan jenis akun yang didukung “Akun di direktori organisasi mana pun dan akun Microsoft pribadi”.
2. Di “Authentication” → “Add a platform” → “Mobile and desktop applications”, tambahkan `http://localhost` sebagai URI pengalihan. Nyalakan “Allow public client flows”. Rahasia klien tidak diperlukan.
3. Di “API permissions”, tambahkan izin terdelegasi *Office 365 Exchange Online* `IMAP.AccessAsUser.All` dan `SMTP.Send` (serta `offline_access`).

Anda masuk dengan peramban yang biasa Anda pakai. Token penyegaran hanya disimpan di gantungan kunci sistem.

## Untuk pengembang

Cara membangun, menguji, mengemas, merilis, dan menerjemahkan dijelaskan di [docs/development.md](docs/development.md) (dalam bahasa Inggris). Lihat [ARCHITECTURE.md](ARCHITECTURE.md) untuk desainnya dan [SECURITY.md](SECURITY.md) untuk cara Fuu menjaga keamanan Anda.

## Lisensi

GPL-3.0-only. Fuu memakai Slint di bawah GPLv3.
