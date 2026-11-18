//! Allow-list CSS filter for `<style>` blocks and `style` attributes.
//!
//! Only declarations whose property is on the allow-list and whose value carries
//! nothing that can load or execute (`url()`, `expression()`, `@import`, escapes…)
//! survive. Positioning that could overlay Fuu's UI is dropped. Of the at-rules
//! only `@media` is kept (its contents are filtered recursively).

const ALLOWED_PROPERTIES: &[&str] = &[
    "color", "background-color", "background", "opacity",
    "font", "font-family", "font-size", "font-weight", "font-style", "font-variant", "line-height",
    "letter-spacing", "word-spacing", "text-align", "text-decoration", "text-decoration-line",
    "text-decoration-color", "text-decoration-style", "text-transform", "text-indent", "white-space",
    "vertical-align", "direction", "word-break", "overflow-wrap", "word-wrap", "hyphens",
    "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
    "padding", "padding-top", "padding-right", "padding-bottom", "padding-left",
    "border", "border-top", "border-right", "border-bottom", "border-left", "border-color", "border-style",
    "border-width", "border-top-color", "border-right-color", "border-bottom-color", "border-left-color",
    "border-top-style", "border-right-style", "border-bottom-style", "border-left-style",
    "border-top-width", "border-right-width", "border-bottom-width", "border-left-width",
    "border-radius", "border-top-left-radius", "border-top-right-radius", "border-bottom-left-radius",
    "border-bottom-right-radius", "border-collapse", "border-spacing",
    "width", "height", "max-width", "min-width", "max-height", "min-height", "box-sizing",
    "display", "visibility", "overflow", "overflow-x", "overflow-y", "float", "clear", "table-layout",
    "caption-side", "empty-cells", "list-style", "list-style-type", "list-style-position",
];

const FORBIDDEN_VALUE_PARTS: &[&str] = &[
    "url(", "image-set(", "expression", "javascript:", "vbscript:", "@import", "\\", "<", ">",
    "behavior", "-moz-binding", "attr(", "var(",
];

fn property_allowed(property: &str) -> bool {
    ALLOWED_PROPERTIES.contains(&property)
}

fn value_safe(property: &str, value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.trim().is_empty() || FORBIDDEN_VALUE_PARTS.iter().any(|bad| lower.contains(bad)) {
        return false;
    }
    // `background` shorthand is allowed only as a colour (no images, gradients).
    if property == "background" && (lower.contains("gradient") || lower.contains('/')) {
        return false;
    }
    true
}

/// Split on `sep` at nesting level 0, respecting quotes and parentheses.
fn split_top_level(input: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None::<char>, 0usize);
    for (i, c) in input.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, c) if c == sep && depth == 0 => {
                parts.push(&input[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts
}

/// Filter a declaration list (`a: b; c: d`). Returns the safe declarations.
pub fn sanitise_declarations(input: &str) -> String {
    let mut out = Vec::new();
    for declaration in split_top_level(input, ';') {
        let Some((property, value)) = declaration.split_once(':') else { continue };
        let property = property.trim().to_ascii_lowercase();
        let value = value.trim();
        if !property_allowed(&property) || !value_safe(&property, value) {
            continue;
        }
        out.push(format!("{property}: {value}"));
    }
    out.join("; ")
}

fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out.replace("<!--", "").replace("-->", "")
}

/// Index of the `}` matching the `{` at `open`, or `None` when unbalanced.
fn matching_brace(input: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut quote = None::<char>;
    for (i, c) in input[open..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '{') => depth += 1,
            (None, '}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

fn selector_safe(selector: &str) -> bool {
    let s = selector.trim();
    !s.is_empty() && !s.contains('\\') && !s.contains('<') && !s.contains('@') && !s.to_ascii_lowercase().contains("expression")
}

fn sanitise_rules(input: &str, depth: usize) -> String {
    let mut out = String::new();
    let mut rest = input.trim();
    while !rest.is_empty() {
        let Some(open) = rest.find('{') else { break };
        let prelude = rest[..open].trim();
        // A statement at-rule (`@import x;`) before the block: skip up to its `;`.
        if let Some(stmt_end) = prelude.find(';') {
            rest = rest[stmt_end + 1..].trim_start();
            continue;
        }
        let Some(close) = matching_brace(rest, open) else { break };
        let block = &rest[open + 1..close];
        if let Some(at) = prelude.strip_prefix('@') {
            let keyword = at.split(|c: char| c.is_whitespace() || c == '(').next().unwrap_or("").to_ascii_lowercase();
            if keyword == "media" && depth < 2 && !prelude.to_ascii_lowercase().contains("url(") {
                let inner = sanitise_rules(block, depth + 1);
                if !inner.is_empty() {
                    out.push_str(&format!("{prelude} {{\n{inner}}}\n"));
                }
            }
        } else if selector_safe(prelude) {
            let declarations = sanitise_declarations(block);
            if !declarations.is_empty() {
                out.push_str(&format!("{prelude} {{ {declarations} }}\n"));
            }
        }
        rest = rest[close + 1..].trim_start();
    }
    out
}

/// Filter a full style sheet.
pub fn sanitise_stylesheet(input: &str) -> String {
    sanitise_rules(&strip_comments(input), 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_safe_declarations() {
        assert_eq!(sanitise_declarations("color: red; font-size: 14px"), "color: red; font-size: 14px");
        assert_eq!(sanitise_declarations("COLOR : #333 ;"), "color: #333");
        assert_eq!(sanitise_declarations("font-family: 'Hiragino Sans', sans-serif"), "font-family: 'Hiragino Sans', sans-serif");
    }

    #[test]
    fn drops_dangerous_declarations() {
        for bad in [
            "background: url(https://t.example/p.gif)",
            "background-image: url(x)",
            "width: expression(alert(1))",
            "behavior: url(x.htc)",
            "-moz-binding: url(x)",
            "position: fixed",
            "z-index: 99999",
            "top: 0",
            "color: \\72 ed",
            "content: 'x'",
            "background: linear-gradient(red, blue)",
            "list-style: url(x)",
        ] {
            assert_eq!(sanitise_declarations(bad), "", "{bad}");
        }
        assert_eq!(sanitise_declarations("color: red; position: absolute; margin: 0"), "color: red; margin: 0");
    }

    #[test]
    fn filters_stylesheets() {
        let css = r#"
            @import url("https://evil.example/x.css");
            @font-face { font-family: X; src: url(https://evil.example/f.woff); }
            /* comment */ p { color: red; background: url(x) }
            .remote { background-image: url(https://t.example/px.gif); }
            @media (max-width: 600px) { td { width: 100%; position: fixed } }
            @keyframes spin { from { opacity: 0 } }
            a { color: #06c }
        "#;
        let out = sanitise_stylesheet(css);
        assert!(!out.contains("url("), "{out}");
        assert!(!out.contains("@import") && !out.contains("@font-face") && !out.contains("@keyframes"), "{out}");
        assert!(out.contains("p { color: red }"), "{out}");
        assert!(out.contains("@media (max-width: 600px)") && out.contains("td { width: 100% }"), "{out}");
        assert!(out.contains("a { color: #06c }"), "{out}");
        assert!(!out.contains(".remote"), "rules left empty are dropped: {out}");
    }

    #[test]
    fn survives_garbage() {
        assert_eq!(sanitise_stylesheet("p { color: red"), "");
        assert_eq!(sanitise_stylesheet("}}}{{{"), "");
        assert_eq!(sanitise_stylesheet("<script>x</script>{color:red}"), "");
    }
}
