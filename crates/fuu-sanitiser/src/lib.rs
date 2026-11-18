//! Security layer between parsing and rendering. Always runs, whichever engines
//! are plugged in. See `SECURITY.md` for the policy this implements.

pub mod css;
pub mod filename;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub use fuu_model::{ParsedMessage, SafeDocument, DOCUMENT_CSP};
use fuu_model::{BodyPart, ManifestEntry, ResourceManifest, SafetyFlags};

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

const ALLOWED_TAGS: &[&str] = &[
    "a", "abbr", "address", "article", "aside", "b", "bdi", "big", "blockquote", "br", "caption",
    "center", "cite", "code", "col", "colgroup", "dd", "del", "details", "dfn", "div", "dl", "dt", "em",
    "figcaption", "figure", "font", "footer", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "i",
    "img", "ins", "kbd", "li", "main", "mark", "nav", "ol", "p", "pre", "q", "rp", "rt", "ruby", "s",
    "samp", "section", "small", "span", "strike", "strong", "sub", "summary", "sup", "table", "tbody",
    "td", "tfoot", "th", "thead", "time", "tr", "tt", "u", "ul", "var", "wbr",
];

const GENERIC_ATTRIBUTES: &[&str] = &[
    "style", "class", "id", "dir", "lang", "title", "align", "valign", "width", "height", "bgcolor",
    "color", "border", "cellpadding", "cellspacing", "colspan", "rowspan", "face", "size", "nowrap",
];

/// Raster formats allowed as resources or `data:` images (no SVG: scriptable).
const RASTER_TYPES: &[&str] = &["image/png", "image/jpeg", "image/jpg", "image/gif", "image/webp"];

const BASE_STYLE: &str = "\
:root { color-scheme: light; }
body { margin: 6px 40px 48px; font: 14px/1.65 system-ui, 'Segoe UI', 'Hiragino Sans', 'Yu Gothic UI', 'Meiryo', sans-serif; color: #1f1f1d; background: #ffffff; overflow-wrap: anywhere; }
img { max-width: 100%; height: auto; }
table { max-width: 100%; }
pre { white-space: pre-wrap; }
a { color: #3b4fd1; }
blockquote { margin: 0 0 0 4px; padding-left: 12px; border-left: 3px solid #d9d6cf; color: #6b6a66; }
.fuu-text { white-space: pre-wrap; font: inherit; margin: 0; }
.fuu-quote { color: #6b6a66; }
";

const TEXT_STYLE: &str = "\
:root { color-scheme: light dark; }
body { margin: 6px 40px 48px; font: 14px/1.7 system-ui, 'Segoe UI', 'Hiragino Sans', 'Yu Gothic UI', 'Meiryo', sans-serif; overflow-wrap: anywhere; }
a { color: #3b4fd1; }
.fuu-text { white-space: pre-wrap; font: inherit; margin: 0; }
.fuu-quote { opacity: 0.65; }
";

/// Extract the contents of `<style>` elements (case-insensitive).
fn extract_styles(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(start) = lower[from..].find("<style") {
        let start = from + start;
        let Some(open_end) = lower[start..].find('>') else { break };
        let content_start = start + open_end + 1;
        let Some(close) = lower[content_start..].find("</style") else { break };
        out.push(html[content_start..content_start + close].to_owned());
        from = content_start + close + 7;
    }
    out
}

/// Undo the character references that can appear in attribute values.
fn unescape_attr(value: &str) -> String {
    value.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">")
}

/// Parse the attributes of one start tag (`<img a=b c="d">`), lower-casing names.
fn tag_attributes(tag: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let body = tag.trim_start_matches('<');
    let mut chars = body.char_indices().peekable();
    // skip the tag name
    while let Some(&(_, c)) = chars.peek() {
        if c.is_whitespace() || c == '>' || c == '/' {
            break;
        }
        chars.next();
    }
    loop {
        while matches!(chars.peek(), Some(&(_, c)) if c.is_whitespace() || c == '/') {
            chars.next();
        }
        let Some(&(start, c)) = chars.peek() else { break };
        if c == '>' {
            break;
        }
        let mut end = start;
        while let Some(&(i, c)) = chars.peek() {
            if c.is_whitespace() || c == '=' || c == '>' || c == '/' {
                break;
            }
            end = i + c.len_utf8();
            chars.next();
        }
        let name = body[start..end].to_ascii_lowercase();
        while matches!(chars.peek(), Some(&(_, c)) if c.is_whitespace()) {
            chars.next();
        }
        let mut value = String::new();
        if matches!(chars.peek(), Some(&(_, '='))) {
            chars.next();
            while matches!(chars.peek(), Some(&(_, c)) if c.is_whitespace()) {
                chars.next();
            }
            match chars.peek().map(|&(_, c)| c) {
                Some(q @ ('"' | '\'')) => {
                    chars.next();
                    for (_, c) in chars.by_ref() {
                        if c == q {
                            break;
                        }
                        value.push(c);
                    }
                }
                _ => {
                    while let Some(&(_, c)) = chars.peek() {
                        if c.is_whitespace() || c == '>' {
                            break;
                        }
                        value.push(c);
                        chars.next();
                    }
                }
            }
        }
        if name.is_empty() {
            chars.next();
            continue;
        }
        attrs.entry(name).or_insert_with(|| unescape_attr(&value));
    }
    attrs
}

fn tiny_length(value: &str) -> bool {
    let v = value.trim().trim_end_matches("px").trim();
    v.parse::<f32>().map(|n| n <= 2.0).unwrap_or(false)
}

/// Remote image URLs that look like tracking pixels: 0–2 px in either dimension,
/// or hidden. These are never fetched, even when remote images are allowed.
fn tracking_pixel_sources(html: &str) -> HashSet<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = HashSet::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<img") {
        let start = from + i;
        let Some(end) = lower[start..].find('>') else { break };
        let attrs = tag_attributes(&html[start..start + end + 1]);
        from = start + end + 1;
        let Some(src) = attrs.get("src") else { continue };
        let style = attrs.get("style").map(|s| s.to_ascii_lowercase().replace(' ', "")).unwrap_or_default();
        let style_tiny = ["width:0", "width:1px", "width:2px", "height:0", "height:1px", "height:2px", "width:1;", "height:1;"]
            .iter()
            .any(|p| style.contains(p));
        let hidden = style.contains("display:none") || style.contains("visibility:hidden") || style.contains("opacity:0");
        let tiny = attrs.get("width").is_some_and(|w| tiny_length(w)) || attrs.get("height").is_some_and(|h| tiny_length(h));
        if tiny || style_tiny || hidden {
            out.insert(src.trim().to_owned());
        }
    }
    out
}

/// Normalise a remote image URL (protocol-relative → https); `None` if not remote.
fn remote_url(value: &str) -> Option<String> {
    let v = value.trim();
    let lower = v.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        Some(v.to_owned())
    } else if lower.starts_with("//") {
        Some(format!("https:{v}"))
    } else {
        None
    }
}

/// Insert fetched remote images into a document: `images[k]` is the image for
/// `remote_images[k]` (`None` when it could not be fetched; that image stays blank).
pub fn with_remote_images(doc: &SafeDocument, images: &[Option<(String, Vec<u8>)>]) -> SafeDocument {
    let mut out = doc.clone();
    for (k, image) in images.iter().enumerate() {
        let Some((mime, data)) = image else { continue };
        if !RASTER_TYPES.contains(&mime.as_str()) {
            continue;
        }
        let index = out.resources.entries.len();
        out.resources.entries.push(ManifestEntry { mime: mime.clone(), data: data.clone() });
        out.html = out.html.replace(&format!("src=\"remote/{k}\""), &format!("src=\"res/{index}\""));
    }
    out.remote_images.clear();
    out
}

fn data_url_is_raster(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let Some(meta) = lower.strip_prefix("data:").and_then(|r| r.split(',').next()) else { return false };
    let mime = meta.split(';').next().unwrap_or("");
    RASTER_TYPES.contains(&mime) && meta.contains(";base64")
}

/// Turn the message into a self-contained document: allow-listed HTML and CSS,
/// `cid:` images rewritten to manifest indices, remote images and every other
/// external load removed. The renderer never sees anything else.
pub fn sanitise(message: &ParsedMessage) -> SafeDocument {
    let html = message.body.iter().find_map(|b| match b {
        BodyPart::Html(h) => Some(h.as_str()),
        _ => None,
    });
    let text = message.body.iter().find_map(|b| match b {
        BodyPart::Text(t) => Some(t.as_str()),
        _ => None,
    });

    // Manifest: raster inline images, addressed by index.
    let mut entries = Vec::new();
    let mut cid_index = HashMap::new();
    for resource in &message.inline {
        if RASTER_TYPES.contains(&resource.mime.as_str()) {
            cid_index.insert(resource.content_id.to_ascii_lowercase(), entries.len());
            entries.push(ManifestEntry { mime: resource.mime.clone(), data: resource.data.clone() });
        }
    }
    let manifest = ResourceManifest { entries };

    match html {
        Some(html) => {
            let (body, flags, remote_images) = sanitise_html(html, &cid_index);
            let css: String = extract_styles(html).iter().map(|s| css::sanitise_stylesheet(s)).collect();
            let document = format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><style>{BASE_STYLE}</style><style>{css}</style></head><body>{body}</body></html>"
            );
            let plain = text.map(str::to_owned).unwrap_or_else(|| html_to_text(html));
            SafeDocument { html: document, text: plain, resources: manifest, flags, remote_images }
        }
        None => {
            let plain = text.unwrap_or_default();
            SafeDocument {
                html: text_document(plain),
                text: plain.to_owned(),
                resources: ResourceManifest::default(),
                flags: SafetyFlags::default(),
                remote_images: Vec::new(),
            }
        }
    }
}

/// Escaped plain text, quoted lines dimmed. No automatic links.
fn text_document(text: &str) -> String {
    let mut body = String::new();
    for line in text.lines() {
        let escaped = escape_html(line);
        if line.trim_start().starts_with('>') {
            body.push_str(&format!("<span class=\"fuu-quote\">{escaped}</span>\n"));
        } else {
            body.push_str(&escaped);
            body.push('\n');
        }
    }
    format!("<!doctype html><html><head><meta charset=\"utf-8\"><style>{TEXT_STYLE}</style></head><body><pre class=\"fuu-text\">{body}</pre></body></html>")
}

fn sanitise_html(html: &str, cid_index: &HashMap<String, usize>) -> (String, SafetyFlags, Vec<String>) {
    let remote_images = Arc::new(AtomicBool::new(false));
    let remote_flag = remote_images.clone();
    let cids: HashMap<String, usize> = cid_index.clone();
    let pixels = tracking_pixel_sources(html);
    let remote_list: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let remote_sink = remote_list.clone();

    let mut tag_attributes: HashMap<&str, HashSet<&str>> = HashMap::new();
    tag_attributes.insert("a", ["href"].into_iter().collect());
    tag_attributes.insert("img", ["src", "alt"].into_iter().collect());

    let cleaned = ammonia::Builder::empty()
        .tags(ALLOWED_TAGS.iter().copied().collect())
        .generic_attributes(GENERIC_ATTRIBUTES.iter().copied().collect())
        .tag_attributes(tag_attributes)
        .url_schemes(["http", "https", "mailto", "cid", "data"].into_iter().collect())
        .url_relative(ammonia::UrlRelative::PassThrough)
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true)
        .attribute_filter(move |element, attribute, value| -> Option<Cow<'_, str>> {
            match (element, attribute) {
                (_, "style") => {
                    let safe = css::sanitise_declarations(value);
                    (!safe.is_empty()).then(|| Cow::Owned(safe))
                }
                ("img", "src") => {
                    let v = value.trim();
                    let lower = v.to_ascii_lowercase();
                    if let Some(cid) = lower.strip_prefix("cid:") {
                        cids.get(cid.trim()).map(|i| Cow::Owned(format!("res/{i}")))
                    } else if lower.starts_with("data:") {
                        data_url_is_raster(v).then(|| Cow::Borrowed(value))
                    } else if let Some(url) = remote_url(v) {
                        remote_flag.store(true, Ordering::Relaxed);
                        if pixels.contains(v) {
                            return None; // tracking-pixel candidate: never fetchable
                        }
                        let mut list = remote_sink.lock().unwrap();
                        let k = list.iter().position(|u| u == &url).unwrap_or_else(|| {
                            list.push(url.clone());
                            list.len() - 1
                        });
                        Some(Cow::Owned(format!("remote/{k}")))
                    } else {
                        None
                    }
                }
                ("a", "href") => {
                    let lower = value.trim().to_ascii_lowercase();
                    (lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:"))
                        .then(|| Cow::Borrowed(value))
                }
                _ => Some(Cow::Borrowed(value)),
            }
        })
        .clean(html)
        .to_string();

    let flags = SafetyFlags { remote_images: remote_images.load(Ordering::Relaxed), ..SafetyFlags::default() };
    let list = remote_list.lock().unwrap().clone();
    (cleaned, flags, list)
}

/// Rough plain-text rendition of HTML (for reply quoting and the text renderer).
pub fn html_to_text(html: &str) -> String {
    // Block-level breaks before the tags disappear.
    let mut marked = html.to_owned();
    for (tag, gap) in [
        ("<br", "\n"), ("<div", "\n"), ("<tr", "\n"), ("<li", "\n"),
        ("<p", "\n\n"), ("<h1", "\n\n"), ("<h2", "\n\n"), ("<h3", "\n\n"), ("<blockquote", "\n\n"), ("</table", "\n\n"),
    ] {
        marked = marked.replace(tag, &format!("{gap}{tag}")).replace(&tag.to_uppercase(), &format!("{gap}{}", tag.to_uppercase()));
    }
    let stripped = ammonia::Builder::empty().clean(&marked).to_string();
    let decoded = stripped
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let mut out = Vec::new();
    for line in decoded.lines().map(str::trim_end) {
        if line.trim().is_empty() && out.last().is_some_and(|l: &String| l.is_empty()) {
            continue;
        }
        out.push(line.trim_start().to_owned());
    }
    out.join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fuu_model::InlineResource;

    fn html_message(html: &str) -> ParsedMessage {
        ParsedMessage { body: vec![BodyPart::Html(html.into())], ..ParsedMessage::default() }
    }

    fn body_of(doc: &SafeDocument) -> &str {
        let start = doc.html.find("<body>").unwrap() + 6;
        &doc.html[start..doc.html.rfind("</body>").unwrap()]
    }

    #[test]
    fn escapes_markup() {
        assert_eq!(escape_html(r#"<img src=x onerror="a">&'"#), "&lt;img src=x onerror=&quot;a&quot;&gt;&amp;&#39;");
    }

    #[test]
    fn removes_scripts_and_active_content() {
        let corpus = r#"
            <script>alert(1)</script>
            <img src=x onerror="alert(2)">
            <a href="javascript:alert(3)">j</a>
            <a href="data:text/html,<script>alert(4)</script>">d</a>
            <a href="vbscript:x">v</a>
            <svg><script>alert(5)</script></svg>
            <iframe src="https://evil.example"></iframe>
            <object data="x"></object><embed src="x">
            <form action="https://evil.example/steal"><input name=p><button>go</button></form>
            <meta http-equiv="refresh" content="0;url=https://evil.example">
            <base href="https://evil.example/">
            <link rel="stylesheet" href="https://evil.example/x.css">
            <div onclick="x" onmouseover="y">ok</div>
            <math><mi>x</mi></math>
        "#;
        let doc = sanitise(&html_message(corpus));
        let body = body_of(&doc).to_ascii_lowercase();
        for bad in ["<script", "onerror", "javascript:", "data:text/html", "vbscript:", "<svg", "<iframe", "<object", "<embed", "<form", "<input", "<button", "<meta", "<base", "<link", "onclick", "onmouseover", "<math", "evil.example"] {
            assert!(!body.contains(bad), "{bad} survived: {body}");
        }
        assert!(body.contains(">ok</div>"));
    }

    #[test]
    fn rewrites_cid_and_blocks_remote_images() {
        let mut m = html_message(r#"<p><img src="cid:Logo@X"><img src="cid:missing@x"><img src="https://t.example/pixel.gif" width="1" height="1"><img src="//t.example/p2.gif"></p>"#);
        m.inline = vec![
            InlineResource { content_id: "logo@x".into(), mime: "image/png".into(), data: vec![1] },
            InlineResource { content_id: "vector@x".into(), mime: "image/svg+xml".into(), data: vec![2] },
        ];
        let doc = sanitise(&m);
        let body = body_of(&doc);
        assert!(body.contains(r#"src="res/0""#), "{body}");
        assert!(!body.contains("t.example"), "{body}");
        assert!(!body.contains("cid:"), "unknown cid left blank: {body}");
        assert!(doc.flags.remote_images);
        // The 1x1 pixel is never listed; the protocol-relative image is, as https.
        assert_eq!(doc.remote_images, vec!["https://t.example/p2.gif".to_owned()]);
        assert!(body.contains(r#"src="remote/0""#), "{body}");
        assert_eq!(doc.resources.entries.len(), 1, "SVG is never a resource");
    }

    #[test]
    fn tracking_pixels_are_never_listed() {
        let doc = sanitise(&html_message(
            r#"<img src="https://a.example/1.gif" width="1" height="1">
               <img src="https://a.example/2.gif" style="display:none">
               <img src="https://a.example/3.gif" style="width: 0px">
               <img src="https://a.example/4.gif" height=2>
               <img src="https://a.example/photo.jpg?x=1&amp;y=2" width="600">
               <img src="https://a.example/photo.jpg?x=1&amp;y=2">"#,
        ));
        assert_eq!(doc.remote_images, vec!["https://a.example/photo.jpg?x=1&y=2".to_owned()], "duplicates share one entry");
        assert!(doc.flags.remote_images);
        let body = body_of(&doc);
        assert_eq!(body.matches(r#"src="remote/0""#).count(), 2);
        assert!(!body.contains("a.example"));
    }

    #[test]
    fn allowed_images_become_manifest_entries() {
        let doc = sanitise(&html_message(r#"<img src="https://a.example/a.png"><img src="https://a.example/b.png"><img src="https://a.example/c.svg">"#));
        assert_eq!(doc.remote_images.len(), 3);
        let fetched = vec![
            Some(("image/png".to_owned(), vec![1, 2])),
            None,
            Some(("image/svg+xml".to_owned(), vec![3])),
        ];
        let allowed = with_remote_images(&doc, &fetched);
        let body = body_of(&allowed);
        assert!(body.contains(r#"src="res/0""#), "{body}");
        assert!(body.contains(r#"src="remote/1""#), "failed fetch stays blank: {body}");
        assert!(body.contains(r#"src="remote/2""#), "SVG is never inserted: {body}");
        assert_eq!(allowed.resources.entries.len(), 1);
        assert!(allowed.remote_images.is_empty());
    }

    #[test]
    fn parses_tag_attributes() {
        let a = tag_attributes(r#"<img SRC='x.png' width=1 alt="a b" data-x>"#);
        assert_eq!(a.get("src").map(String::as_str), Some("x.png"));
        assert_eq!(a.get("width").map(String::as_str), Some("1"));
        assert_eq!(a.get("alt").map(String::as_str), Some("a b"));
        assert!(a.contains_key("data-x"));
    }

    #[test]
    fn data_urls_raster_only() {
        let doc = sanitise(&html_message(r#"<img src="data:image/png;base64,iVBORw0KGgo="><img src="data:image/svg+xml;base64,PHN2Zz4="><img src="data:text/html;base64,PHNjcmlwdD4=">"#));
        let body = body_of(&doc);
        assert_eq!(body.matches("<img").count(), 3);
        assert_eq!(body.matches("data:image/png").count(), 1, "{body}");
        assert!(!body.contains("svg") && !body.contains("text/html"), "{body}");
        assert!(!doc.flags.remote_images);
    }

    #[test]
    fn keeps_links_and_safe_styles() {
        let doc = sanitise(&html_message(
            r#"<style>@import url(x); p { color: red; background: url(https://t.example/a.png) } .x { position: fixed }</style>
               <p style="color: blue; position: absolute; background-image: url(x)">hi <a href="https://example.org/">site</a> <a href="mailto:a@example.org">mail</a></p>"#,
        ));
        let body = body_of(&doc);
        assert!(body.contains(r#"href="https://example.org/""#));
        assert!(body.contains(r#"href="mailto:a@example.org""#));
        assert!(body.contains(r#"style="color: blue""#), "{body}");
        assert!(body.contains("noopener"));
        let head = &doc.html[..doc.html.find("<body>").unwrap()];
        assert!(head.contains("p { color: red }"), "{head}");
        assert!(!head.contains("url(") && !head.contains("@import") && !head.contains("fixed"), "{head}");
    }

    #[test]
    fn text_messages_are_escaped_without_links() {
        let m = ParsedMessage { body: vec![BodyPart::Text("<b>x</b> https://example.org\n> quoted".into())], ..ParsedMessage::default() };
        let doc = sanitise(&m);
        assert!(doc.html.contains("&lt;b&gt;x&lt;/b&gt;"));
        assert!(!doc.html.contains("<a "), "no automatic links");
        assert!(doc.html.contains("fuu-quote"));
        assert_eq!(doc.text, "<b>x</b> https://example.org\n> quoted");
    }

    #[test]
    fn html_to_text_keeps_structure() {
        let t = html_to_text("<p>Hello&nbsp;<b>world</b></p><p>Line 2<br>Line 3</p><script>x()</script>");
        assert_eq!(t, "Hello world\n\nLine 2\nLine 3");
    }

    #[test]
    fn csp_blocks_scripts_and_remote_loads() {
        assert!(DOCUMENT_CSP.contains("script-src 'none'"));
        assert!(DOCUMENT_CSP.contains("default-src 'none'"));
        assert!(!DOCUMENT_CSP.contains("http"));
    }
}
