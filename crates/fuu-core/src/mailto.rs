//! `mailto:` URLs (RFC 6068) turned into the fields of a new message.
//!
//! The URL is untrusted: it comes from a link in a mail, a web page, or the
//! operating system on behalf of any program. Only the addresses, `subject`
//! and `body` are taken; every other header is ignored, in particular
//! `attach`/`attachment` (a known way to make mail clients send local files).
//! Header fields lose control characters (no header injection) and every
//! field is capped. Nothing is ever sent without the user pressing Send.

/// Longest URL accepted at all.
pub const MAX_URL: usize = 64 * 1024;
const MAX_ADDRESSES: usize = 4000;
const MAX_SUBJECT: usize = 998;
const MAX_BODY: usize = 32 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mailto {
    /// Comma-separated address lists, as typed in the composer's fields.
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    /// Plain text with `\n` line breaks.
    pub body: String,
}

/// Parse a `mailto:` URL; `None` when it is not one (or is too long).
pub fn parse(url: &str) -> Option<Mailto> {
    let url = url.trim();
    if url.len() > MAX_URL || url.len() < 7 || !url[..7].eq_ignore_ascii_case("mailto:") {
        return None;
    }
    let rest = &url[7..];
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    // A fragment is not part of the message.
    let query = query.split('#').next().unwrap_or_default();
    let path = path.split('#').next().unwrap_or_default();

    let (mut to, mut cc, mut bcc) = (Vec::new(), Vec::new(), Vec::new());
    let (mut subject, mut body) = (None, None);
    push(&mut to, &decode(path));
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode(value);
        match decode(key).to_ascii_lowercase().as_str() {
            "to" => push(&mut to, &value),
            "cc" => push(&mut cc, &value),
            "bcc" => push(&mut bcc, &value),
            "subject" if subject.is_none() => subject = Some(value),
            "body" if body.is_none() => body = Some(value),
            // attach, attachment, in-reply-to, from, … are ignored on purpose.
            _ => {}
        }
    }
    Some(Mailto {
        to: list(&to),
        cc: list(&cc),
        bcc: list(&bcc),
        subject: cap(&header_text(&subject.unwrap_or_default()), MAX_SUBJECT),
        body: cap(&body.unwrap_or_default().replace("\r\n", "\n").replace('\r', "\n"), MAX_BODY),
    })
}

/// Percent-decoding as in URLs, not forms: `+` stays a plus (it is common in
/// addresses such as `user+tag@example.com`). Invalid UTF-8 is replaced.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Header text: no control or bidirectional-override characters.
fn header_text(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}'))
        .collect::<String>()
        .trim()
        .to_owned()
}

fn push(list: &mut Vec<String>, value: &str) {
    list.extend(header_text(value).split(',').map(str::trim).filter(|a| !a.is_empty()).map(str::to_owned));
}

fn list(addresses: &[String]) -> String {
    cap(&addresses.join(", "), MAX_ADDRESSES)
}

fn cap(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_address() {
        let m = parse("mailto:someone@example.com").unwrap();
        assert_eq!(m.to, "someone@example.com");
        assert_eq!(m, Mailto { to: "someone@example.com".into(), ..Default::default() });
        assert!(parse("MAILTO:a@b.example").is_some());
        assert!(parse("https://example.com").is_none());
        assert!(parse("mailto").is_none());
    }

    #[test]
    fn fields_and_percent_decoding() {
        let m = parse("mailto:a@example.com,b@example.com?cc=c@example.com&BCC=d@example.com&subject=%E4%BC%9A%E8%AD%B0%20%E3%81%AE%E4%BB%B6&body=line1%0D%0Aline2&to=e@example.com").unwrap();
        assert_eq!(m.to, "a@example.com, b@example.com, e@example.com");
        assert_eq!(m.cc, "c@example.com");
        assert_eq!(m.bcc, "d@example.com");
        assert_eq!(m.subject, "会議 の件");
        assert_eq!(m.body, "line1\nline2");
    }

    #[test]
    fn plus_is_not_a_space() {
        let m = parse("mailto:user+tag@example.com?subject=a+b").unwrap();
        assert_eq!(m.to, "user+tag@example.com");
        assert_eq!(m.subject, "a+b");
    }

    #[test]
    fn no_header_injection_or_attachments() {
        let m = parse("mailto:a@example.com%0D%0ABcc:evil@example.com?subject=hi%0D%0AX-Evil:%201&attach=C:%5Csecret.txt&attachment=/etc/passwd").unwrap();
        assert!(!m.to.contains('\n') && !m.to.contains('\r'));
        assert!(!m.subject.contains('\n') && !m.subject.contains('\r'));
        assert_eq!(m.subject, "hi  X-Evil: 1");
        assert!(m.body.is_empty() && m.cc.is_empty() && m.bcc.is_empty());
    }

    #[test]
    fn broken_escapes_and_limits() {
        assert_eq!(parse("mailto:a@example.com?subject=100%25%zz%").unwrap().subject, "100%%zz%");
        assert_eq!(parse("mailto:?subject=%FF").unwrap().subject, "\u{FFFD}");
        let long = format!("mailto:a@example.com?body={}", "x".repeat(MAX_BODY + 10));
        assert_eq!(parse(&long).unwrap().body.len(), MAX_BODY);
        assert!(parse(&format!("mailto:{}", "a".repeat(MAX_URL))).is_none());
        assert_eq!(parse("mailto:a@example.com?subject=%E2%80%AEevil").unwrap().subject, "evil");
    }
}
