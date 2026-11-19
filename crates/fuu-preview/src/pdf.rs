//! PDF: pages rasterised with hayro (pure Rust, no unsafe code) and shown as
//! images. Fonts a PDF does not embed come from hayro's built-in standard
//! fonts, or for Chinese, Japanese and Korean text from the OS's own fonts.

use std::sync::{Arc, OnceLock};

use hayro::hayro_interpret::font::{FallbackFontQuery, FontData, FontQuery};
use hayro::hayro_interpret::hayro_cmap::CidFamily;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};

use crate::{html, Output, PreviewError};

/// Pages rendered; longer documents show a note.
const MAX_PAGES: usize = 15;
/// Width of a rendered page in pixels (sharp at 1.5× on a typical reader pane).
const PAGE_WIDTH: f32 = 1200.0;
/// Upper bound on one page's pixels (very tall pages are scaled to fit it).
const MAX_PAGE_PIXELS: f32 = 6_000_000.0;

#[derive(Clone, Copy)]
enum Script {
    Japanese,
    Chinese,
    Korean,
}

/// Candidate OS font files per script (first existing wins), regular and bold.
fn system_font_paths(script: Script, bold: bool) -> &'static [&'static str] {
    match (script, bold) {
        #[cfg(windows)]
        (Script::Japanese, false) => &[r"C:\Windows\Fonts\YuGothM.ttc", r"C:\Windows\Fonts\meiryo.ttc", r"C:\Windows\Fonts\msgothic.ttc"],
        #[cfg(windows)]
        (Script::Japanese, true) => &[r"C:\Windows\Fonts\YuGothB.ttc", r"C:\Windows\Fonts\meiryob.ttc", r"C:\Windows\Fonts\msgothic.ttc"],
        #[cfg(windows)]
        (Script::Chinese, _) => &[r"C:\Windows\Fonts\msyh.ttc", r"C:\Windows\Fonts\simsun.ttc"],
        #[cfg(windows)]
        (Script::Korean, _) => &[r"C:\Windows\Fonts\malgun.ttf"],
        #[cfg(target_os = "macos")]
        (Script::Japanese, false) => &["/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc"],
        #[cfg(target_os = "macos")]
        (Script::Japanese, true) => &["/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc", "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"],
        #[cfg(target_os = "macos")]
        (Script::Chinese, _) => &["/System/Library/Fonts/PingFang.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc"],
        #[cfg(target_os = "macos")]
        (Script::Korean, _) => &["/System/Library/Fonts/AppleSDGothicNeo.ttc"],
        #[allow(unreachable_patterns)]
        _ => &[],
    }
}

/// OS font for a script, read once and shared.
fn system_font(script: Script, bold: bool) -> Option<FontData> {
    static CACHE: OnceLock<std::sync::Mutex<Vec<((u8, bool), Option<FontData>)>>> = OnceLock::new();
    let key = (script as u8, bold);
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = cache.lock().ok()?;
    if let Some((_, font)) = cache.iter().find(|(k, _)| *k == key) {
        return font.clone();
    }
    let font = system_font_paths(script, bold)
        .iter()
        .find_map(|p| std::fs::read(p).ok())
        .map(|data| Arc::new(data) as FontData);
    cache.push((key, font.clone()));
    font
}

/// Which CJK script a missing font is for: from its character collection, else
/// from familiar Japanese / Chinese / Korean font names.
fn script_of(query: &FallbackFontQuery) -> Option<Script> {
    if let Some(cc) = &query.character_collection {
        return match cc.family {
            CidFamily::AdobeJapan1 => Some(Script::Japanese),
            CidFamily::AdobeGB1 | CidFamily::AdobeCNS1 => Some(Script::Chinese),
            CidFamily::AdobeKorea1 => Some(Script::Korean),
            _ => None,
        };
    }
    let name = [&query.post_script_name, &query.font_name, &query.font_family]
        .iter()
        .filter_map(|n| n.as_deref())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    const JAPANESE: &[&str] = &["gothic", "mincho", "meiryo", "yugoth", "yumin", "hiragino", "kozuka", "ipa", "osaka", "ms-p", "msp"];
    if JAPANESE.iter().any(|n| name.contains(n)) || name.contains('\u{30b4}') {
        return Some(Script::Japanese);
    }
    if ["simsun", "simhei", "yahei", "pingfang", "songti", "mingliu", "kaiti"].iter().any(|n| name.contains(n)) {
        return Some(Script::Chinese);
    }
    if ["malgun", "batang", "gulim", "dotum", "nanum"].iter().any(|n| name.contains(n)) {
        return Some(Script::Korean);
    }
    None
}

fn settings() -> InterpreterSettings {
    InterpreterSettings {
        font_resolver: Arc::new(|query| match query {
            FontQuery::Standard(s) => Some(s.get_font_data()),
            FontQuery::Fallback(f) => {
                if let Some(font) = script_of(f).and_then(|s| system_font(s, f.is_bold)) {
                    return Some((font, 0));
                }
                Some(f.pick_standard_font().get_font_data())
            }
        }),
        ..InterpreterSettings::default()
    }
}

pub(crate) fn preview(data: &[u8]) -> Result<Output, PreviewError> {
    let pdf = Pdf::new(data.to_vec()).map_err(|e| {
        PreviewError::Unreadable(match e {
            hayro::hayro_syntax::LoadPdfError::Decryption(_) => "パスワードで保護されています".into(),
            _ => "PDF として読み取れません".into(),
        })
    })?;
    let pages = pdf.pages();
    let total = pages.len();
    if total == 0 {
        return Err(PreviewError::Unreadable("ページがありません".into()));
    }
    let settings = settings();
    let cache = RenderCache::new();
    let mut out = Output::default();
    let shown = total.min(MAX_PAGES);
    out.html.push_str(&html::note(&if total > shown {
        format!("全 {total} ページのうち、最初の {shown} ページを表示しています")
    } else {
        format!("{total} ページ")
    }));
    for page in pages.iter().take(shown) {
        let (w, h) = page.render_dimensions();
        if !(w.is_finite() && h.is_finite()) || w < 1.0 || h < 1.0 {
            continue;
        }
        let mut scale = PAGE_WIDTH / w;
        if w * h * scale * scale > MAX_PAGE_PIXELS {
            scale = (MAX_PAGE_PIXELS / (w * h)).sqrt();
        }
        // u16 pixmap sides: keep well inside.
        scale = scale.min(8000.0 / w.max(h));
        let pixmap_settings = PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE };
        let pixmap = render(page, &cache, &settings, &RenderSettings::default(), &pixmap_settings);
        let png = pixmap.into_png().map_err(|e| PreviewError::Unreadable(format!("{e:?}")))?;
        let src = out.add_image("image/png", png);
        out.html.push_str(&format!("<img class=\"fuu-page\" src=\"{src}\" alt=\"\">"));
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A one-page PDF drawing `text` (UTF-16 hex) with a non-embedded font.
    /// `japanese`: an Adobe-Japan1 CID font (like most Japanese business PDFs).
    pub(crate) fn sample_pdf(japanese: bool) -> Vec<u8> {
        let font = if japanese {
            "<< /Type /Font /Subtype /Type0 /BaseFont /MS-Gothic /Encoding /UniJIS-UCS2-H /DescendantFonts [6 0 R] >>"
        } else {
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
        };
        let text = if japanese { "<3042304430463048304A>" } else { "(Hello PDF)" };
        let content = format!("BT /F1 48 Tf 40 100 Td {text} Tj ET");
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            font.to_owned(),
        ];
        if japanese {
            objects.push("<< /Type /Font /Subtype /CIDFontType0 /BaseFont /MS-Gothic /CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 2 >> /FontDescriptor 7 0 R /DW 1000 >>".to_owned());
            objects.push("<< /Type /FontDescriptor /FontName /MS-Gothic /Flags 4 /FontBBox [0 -140 1000 880] /ItalicAngle 0 /Ascent 880 /Descent -120 /CapHeight 700 /StemV 80 >>".to_owned());
        }
        let mut pdf = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
        }
        let xref = pdf.len();
        pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for o in offsets {
            pdf.extend(format!("{o:010} 00000 n \n").bytes());
        }
        pdf.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
        pdf
    }

    /// Fraction of clearly dark pixels in a rendered page.
    fn ink(png: &[u8]) -> f64 {
        let page = image::load_from_memory(png).unwrap().to_luma8();
        let dark = page.pixels().filter(|p| p.0[0] < 128).count();
        dark as f64 / page.pixels().count() as f64
    }

    #[test]
    fn renders_pages_as_images() {
        let out = preview(&sample_pdf(false)).unwrap();
        assert_eq!(out.images.len(), 1);
        assert!(out.html.contains("1 ページ"));
        let page = image::load_from_memory(&out.images[0].data).unwrap();
        assert_eq!(page.width(), 1200);
        assert!(ink(&out.images[0].data) > 0.005, "text drawn");
    }

    #[test]
    fn japanese_text_without_embedded_fonts_still_draws() {
        let out = preview(&sample_pdf(true)).unwrap();
        if let Some(path) = std::env::var_os("FUU_PREVIEW_DUMP") {
            std::fs::write(path, &out.images[0].data).unwrap();
        }
        let coverage = ink(&out.images[0].data);
        assert!(coverage > 0.005, "あいうえお should be drawn with an OS font, ink {coverage}");
    }

    #[test]
    fn damaged_files_are_errors_not_panics() {
        assert!(preview(b"%PDF-1.7\n garbage").is_err());
        assert!(preview(b"").is_err());
        let mut cut = sample_pdf(false);
        cut.truncate(cut.len() / 2);
        let _ = preview(&cut);
    }
}
