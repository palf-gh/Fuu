//! Small HTML helpers shared by the converters. The output only uses tags,
//! attributes and CSS properties the sanitiser keeps.

/// Preview stylesheet (filtered by the sanitiser like any mail `<style>`).
pub const STYLE: &str = "\
body { margin: 18px 28px 48px; }
.fuu-note { color: #6b6a66; font-size: 12px; margin: 0 0 14px; }
.fuu-page { display: block; width: 100%; max-width: 100%; height: auto; margin: 0 auto 16px; border: 1px solid #e4e2dc; }
.fuu-image { display: block; max-width: 100%; height: auto; margin: 0 auto; }
.fuu-doc p { margin: 0 0 6px; }
.fuu-doc table { border-collapse: separate; border-spacing: 0; border-top: 1px solid #c9c6bd; border-left: 1px solid #c9c6bd; margin: 8px 0 12px; }
.fuu-doc td { border-right: 1px solid #c9c6bd; border-bottom: 1px solid #c9c6bd; padding: 3px 6px; vertical-align: top; }
.fuu-sheet { border-collapse: separate; border-spacing: 0; border-top: 1px solid #d9d6cf; border-left: 1px solid #d9d6cf; font-size: 12px; margin: 6px 0 22px; }
.fuu-sheet td, .fuu-sheet th { border-right: 1px solid #d9d6cf; border-bottom: 1px solid #d9d6cf; padding: 2px 6px; white-space: nowrap; }
.fuu-sheet th { background-color: #f1f0ec; color: #6b6a66; font-weight: normal; }
.fuu-sheet td.num { text-align: right; }
.fuu-slide { border: 1px solid #d9d6cf; border-radius: 8px; padding: 22px 28px; margin: 0 0 18px; background-color: #ffffff; }
.fuu-slide-no { color: #a3a19b; font-size: 11px; margin: 0 0 8px; }
.fuu-slide h2 { font-size: 22px; margin: 0 0 12px; }
.fuu-slide p { margin: 0 0 6px; }
.fuu-slide img { max-width: 100%; height: auto; margin: 6px 0; }
.fuu-tabs { color: #6b6a66; font-size: 13px; margin: 18px 0 6px; font-weight: bold; }
";

/// Escape text for HTML content and attribute values.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            // Control characters other than tab/newline never reach the document.
            c if c.is_control() && c != '\t' && c != '\n' => {}
            c => out.push(c),
        }
    }
    out
}

/// A note line above the preview (truncation, missing pieces).
pub fn note(text: &str) -> String {
    format!("<p class=\"fuu-note\">{}</p>", escape(text))
}

/// `#RRGGBB` from a 6-digit hex value, or `None` (also for "auto").
pub fn colour(hex: &str) -> Option<String> {
    let hex = hex.trim();
    (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("#{}", hex.to_ascii_lowercase()))
}

/// Spreadsheet column name: 0 → A, 26 → AA.
pub fn column_name(mut index: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + (index % 26) as u8);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup_and_drops_controls() {
        assert_eq!(escape("<a href=\"x\">&'\u{0}\u{202e}"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;\u{202e}");
    }

    #[test]
    fn columns_and_colours() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(701), "ZZ");
        assert_eq!(colour("FF0000").as_deref(), Some("#ff0000"));
        assert_eq!(colour("auto"), None);
        assert_eq!(colour("red;x"), None);
    }
}
