//! Security layer between parsing and rendering. Always runs, whichever engines
//! are plugged in. The allow-list implementation lands in M2; see `SECURITY.md`.

pub use fuu_model::{ParsedMessage, SafeDocument, DOCUMENT_CSP};

/// Escape text for safe inclusion in HTML element content or quoted attributes.
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup() {
        assert_eq!(
            escape_html(r#"<img src=x onerror="a">&'"#),
            "&lt;img src=x onerror=&quot;a&quot;&gt;&amp;&#39;"
        );
    }

    #[test]
    fn csp_blocks_scripts_and_remote_loads() {
        assert!(DOCUMENT_CSP.contains("script-src 'none'"));
        assert!(DOCUMENT_CSP.contains("default-src 'none'"));
        assert!(!DOCUMENT_CSP.contains("http"));
    }
}
