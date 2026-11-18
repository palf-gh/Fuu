//! Building outgoing mail: recipients, reply/forward conventions, quoting and the
//! final RFC 5322 bytes (UTF-8, MIME, threading headers).

use fuu_model::{Address, Headers};
use mail_builder::headers::address::Address as MbAddress;
use mail_builder::MessageBuilder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    pub name: Option<String>,
    pub email: String,
}

impl Recipient {
    pub fn display(&self) -> String {
        match &self.name {
            Some(n) => format!("{n} <{}>", self.email),
            None => self.email.clone(),
        }
    }
}

impl From<&Address> for Recipient {
    fn from(a: &Address) -> Self {
        Recipient { name: a.name.clone(), email: a.email.clone() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingAttachment {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outgoing {
    pub from_name: String,
    pub from_email: String,
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub bcc: Vec<Recipient>,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<OutgoingAttachment>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// Also send an HTML alternative (multipart/alternative).
    pub html: bool,
    /// HTML body fragment from the rich editor (inline styles only). When absent,
    /// the HTML alternative is generated from `body`.
    pub html_body: Option<String>,
}

impl Outgoing {
    /// Every envelope recipient (To, CC and BCC), de-duplicated.
    pub fn envelope_recipients(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in self.to.iter().chain(&self.cc).chain(&self.bcc) {
            if !out.iter().any(|e| e.eq_ignore_ascii_case(&r.email)) {
                out.push(r.email.clone());
            }
        }
        out
    }
}

/// Very small address syntax check: `local@domain`, no spaces or angle brackets.
pub fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.rsplit_once('@') else { return false };
    !local.is_empty()
        && !domain.is_empty()
        && !email.chars().any(|c| c.is_whitespace() || matches!(c, '<' | '>' | ',' | ';' | '"'))
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

/// Parse `a@x, Name <b@y>; "Last, First" <c@z>` into recipients.
pub fn parse_recipients(input: &str) -> Result<Vec<Recipient>, String> {
    let mut parts = Vec::new();
    let (mut current, mut quoted, mut angle) = (String::new(), false, false);
    for c in input.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            '<' if !quoted => {
                angle = true;
                current.push(c);
            }
            '>' if !quoted => {
                angle = false;
                current.push(c);
            }
            ',' | ';' | '、' | '\n' if !quoted && !angle => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    let mut out = Vec::new();
    for part in parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let (name, email) = match (part.rfind('<'), part.rfind('>')) {
            (Some(open), Some(close)) if close > open => {
                let name = part[..open].trim().trim_matches('"').trim();
                (if name.is_empty() { None } else { Some(name.to_owned()) }, part[open + 1..close].trim().to_owned())
            }
            _ => (None, part.to_owned()),
        };
        if !valid_email(&email) {
            return Err(format!("メールアドレスが正しくありません: {part}"));
        }
        out.push(Recipient { name, email });
    }
    Ok(out)
}

/// `Re: subject` without stacking prefixes (Re:, RE:, Re[2]:, AW:, 返信:, Fwd: kept).
pub fn reply_subject(subject: &str) -> String {
    let mut rest = subject.trim();
    loop {
        let lower = rest.to_lowercase();
        let stripped = ["re:", "re：", "aw:", "sv:", "返信:", "返信："]
            .iter()
            .find(|p| lower.starts_with(*p))
            .map(|p| rest[p.len()..].trim_start())
            .or_else(|| {
                // Re[2]: / Re(2):
                let b = lower.strip_prefix("re")?;
                let close_char = match b.chars().next()? {
                    '[' => ']',
                    '(' => ')',
                    _ => return None,
                };
                let close = b.find(close_char)?;
                b[close + 1..].starts_with(':').then(|| rest[2 + close + 2..].trim_start())
            });
        match stripped {
            Some(s) => rest = s,
            None => break,
        }
    }
    format!("Re: {rest}")
}

pub fn forward_subject(subject: &str) -> String {
    let s = subject.trim();
    let lower = s.to_lowercase();
    if lower.starts_with("fwd:") || lower.starts_with("fw:") {
        s.to_owned()
    } else {
        format!("Fwd: {s}")
    }
}

fn is_own(email: &str, own: &[String]) -> bool {
    own.iter().any(|o| o.eq_ignore_ascii_case(email))
}

/// Recipients for a reply: Reply-To (or From); for reply-all also the original
/// To and CC, without the user's own addresses or duplicates.
pub fn reply_recipients(original: &Headers, own: &[String], all: bool) -> (Vec<Recipient>, Vec<Recipient>) {
    let primary: Vec<Recipient> = if !original.reply_to.is_empty() { &original.reply_to } else { &original.from }
        .iter()
        .map(Recipient::from)
        .collect();
    // Replying to one's own sent message: answer its recipients instead.
    let primary = if primary.iter().all(|r| is_own(&r.email, own)) && !original.to.is_empty() {
        original.to.iter().map(Recipient::from).collect()
    } else {
        primary
    };
    if !all {
        return (primary, Vec::new());
    }
    let mut seen: Vec<String> = primary.iter().map(|r| r.email.to_ascii_lowercase()).collect();
    let mut cc = Vec::new();
    for a in original.to.iter().chain(&original.cc) {
        let lower = a.email.to_ascii_lowercase();
        if is_own(&a.email, own) || seen.contains(&lower) {
            continue;
        }
        seen.push(lower);
        cc.push(Recipient::from(a));
    }
    (primary, cc)
}

/// The quoted original below a reply, Japanese-mail style (top-posting).
pub fn quote(original_text: &str, from: &str, date: &str) -> String {
    let mut out = String::new();
    if date.is_empty() {
        out.push_str(&format!("{from} さんは書きました:\n"));
    } else {
        out.push_str(&format!("{date}、{from} さんは書きました:\n"));
    }
    for line in original_text.trim_end().lines() {
        if line.starts_with('>') {
            out.push('>');
        } else {
            out.push_str("> ");
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Labels of the original-message header block, in order (bolded by the editor).
pub const HEADER_LABELS: [&str; 5] = ["差出人:", "送信:", "宛先:", "CC:", "件名:"];

/// Long Japanese date for the original-message header, in local time:
/// `2026 年 11 月 18 日 (水曜日) 20:03`.
pub fn header_date(timestamp: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(d) = Local.timestamp_opt(timestamp, 0).single() else { return String::new() };
    const DAYS: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];
    format!(
        "{} 年 {} 月 {} 日 ({}曜日) {}",
        d.year(),
        d.month(),
        d.day(),
        DAYS[d.weekday().num_days_from_monday() as usize],
        d.format("%H:%M")
    )
}

/// Outlook-style header block of the original message, placed under a rule above
/// the original text in replies and forwards.
pub fn original_header(original: &Headers) -> String {
    let one = |a: &Address| match a.name.as_deref().filter(|n| !n.trim().is_empty()) {
        Some(n) => format!("\"{n}\" <{}>", a.email),
        None => a.email.clone(),
    };
    let list = |v: &[Address]| v.iter().map(one).collect::<Vec<_>>().join("; ");
    let mut out = format!("差出人: {}\n", list(&original.from));
    if let Some(date) = original.date.map(header_date).filter(|d| !d.is_empty()) {
        out.push_str(&format!("送信: {date}\n"));
    }
    if !original.to.is_empty() {
        out.push_str(&format!("宛先: {}\n", list(&original.to)));
    }
    if !original.cc.is_empty() {
        out.push_str(&format!("CC: {}\n", list(&original.cc)));
    }
    out.push_str(&format!("件名: {}\n", original.subject));
    out
}

/// The forwarded-message block appended to a forward.
pub fn forward_block(original: &Headers, original_text: &str, date: &str) -> String {
    let list = |v: &[Address]| v.iter().map(|a| Recipient::from(a).display()).collect::<Vec<_>>().join(", ");
    let mut out = String::from("---------- 転送メッセージ ----------\n");
    out.push_str(&format!("差出人: {}\n", list(&original.from)));
    if !date.is_empty() {
        out.push_str(&format!("日時: {date}\n"));
    }
    out.push_str(&format!("件名: {}\n", original.subject));
    out.push_str(&format!("宛先: {}\n", list(&original.to)));
    if !original.cc.is_empty() {
        out.push_str(&format!("CC: {}\n", list(&original.cc)));
    }
    out.push('\n');
    out.push_str(original_text.trim_end());
    out.push('\n');
    out
}


const FONT_STACK: &str = "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Yu Gothic UI', Meiryo, sans-serif";

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape a line and turn bare http(s) URLs into links.
fn linkify(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    loop {
        let next = ["https://", "http://"].iter().filter_map(|p| rest.find(p)).min();
        let Some(start) = next else {
            out.push_str(&escape(rest));
            return out;
        };
        out.push_str(&escape(&rest[..start]));
        let url_len = rest[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '「' | '」' | '（' | '）' | '、' | '。'))
            .unwrap_or(rest.len() - start);
        let mut url = &rest[start..start + url_len];
        // Trailing punctuation is usually not part of the URL.
        while let Some(last) = url.chars().last() {
            if matches!(last, '.' | ',' | ')' | '!' | '?' | ';' | ':') {
                url = &url[..url.len() - last.len_utf8()];
            } else {
                break;
            }
        }
        let escaped = escape(url);
        out.push_str(&format!("<a href=\"{escaped}\" style=\"color:#3b4fd1;\">{escaped}</a>"));
        rest = &rest[start + url.len()..];
    }
}

/// Quote depth of a line (`> > text` → 2) and the text after the markers.
fn quote_depth(line: &str) -> (usize, &str) {
    let mut depth = 0;
    let mut rest = line;
    loop {
        let trimmed = rest.trim_start_matches(' ');
        match trimmed.strip_prefix('>') {
            Some(r) => {
                depth += 1;
                rest = r.strip_prefix(' ').unwrap_or(r);
            }
            None => return (depth, rest),
        }
    }
}

/// HTML rendition of a plain-text body for the text/html alternative: paragraphs
/// and line breaks kept, `>` quotes as blockquotes, the `-- ` signature muted,
/// URLs linked. Inline styles only (many mail clients strip `<style>`).
pub fn text_to_html(body: &str) -> String {
    let body = body.replace("\r\n", "\n");
    let (main, signature) = match body.find("\n-- \n").or_else(|| body.starts_with("-- \n").then_some(0)) {
        Some(i) => {
            let sig_start = if i == 0 { 4 } else { i + 5 };
            (&body[..i], Some(&body[sig_start..]))
        }
        None => (body.as_str(), None),
    };
    let mut html = String::new();
    let mut depth = 0usize;
    for line in main.lines() {
        let (d, text) = quote_depth(line);
        while depth < d {
            html.push_str("<blockquote style=\"margin:0 0 0 2px;padding:0 0 0 12px;border-left:3px solid #d9d6cf;color:#6b6a66;\">");
            depth += 1;
        }
        while depth > d {
            html.push_str("</blockquote>");
            depth -= 1;
        }
        if text.is_empty() {
            html.push_str("<br>");
        } else {
            html.push_str(&format!("<div>{}</div>", linkify(text)));
        }
    }
    while depth > 0 {
        html.push_str("</blockquote>");
        depth -= 1;
    }
    if let Some(sig) = signature {
        html.push_str("<div style=\"color:#8a877f;margin-top:16px;\">-- <br>");
        html.push_str(&sig.trim_end().lines().map(linkify).collect::<Vec<_>>().join("<br>"));
        html.push_str("</div>");
    }
    wrap_html(&html)
}

/// Wrap an HTML body fragment in a complete document with Fuu's base text style.
pub fn wrap_html(fragment: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body><div style=\"font-family:{FONT_STACK};font-size:14px;line-height:1.6;color:#1f1f1d;\">{fragment}</div></body></html>"
    )
}

/// MIME type from a file name (for attachments picked from disk).
pub fn guess_mime(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("txt" | "log" | "md") => "text/plain",
        Some("csv") => "text/csv",
        Some("html" | "htm") => "text/html",
        Some("zip") => "application/zip",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xls") => "application/vnd.ms-excel",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("ppt") => "application/vnd.ms-powerpoint",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("eml") => "message/rfc822",
        _ => "application/octet-stream",
    }
}

fn mb_list(list: &[Recipient]) -> MbAddress<'static> {
    MbAddress::new_list(
        list.iter().map(|r| MbAddress::new_address(r.name.clone(), r.email.clone())).collect(),
    )
}

/// A new Message-ID at the sender's domain.
pub fn new_message_id(from_email: &str) -> String {
    let domain = from_email.rsplit_once('@').map(|(_, d)| d).filter(|d| !d.is_empty()).unwrap_or("fuu.invalid");
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    format!("{:x}.{:x}@{domain}", nanos, std::process::id())
}

/// Render the message (BCC is never written to the headers).
pub fn build(outgoing: &Outgoing) -> Result<(String, Vec<u8>), String> {
    if outgoing.envelope_recipients().is_empty() {
        return Err("宛先を入力してください".into());
    }
    if !valid_email(&outgoing.from_email) {
        return Err("差出人のアドレスが正しくありません".into());
    }
    let message_id = new_message_id(&outgoing.from_email);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default();
    let mut builder = MessageBuilder::new()
        .from(MbAddress::new_address(
            (!outgoing.from_name.is_empty()).then(|| outgoing.from_name.clone()),
            outgoing.from_email.clone(),
        ))
        .subject(outgoing.subject.clone())
        .date(now)
        .message_id(message_id.clone())
        .text_body(outgoing.body.replace("\r\n", "\n"));
    if outgoing.html {
        let html = match &outgoing.html_body {
            Some(fragment) => wrap_html(fragment),
            None => text_to_html(&outgoing.body),
        };
        builder = builder.html_body(html);
    }
    if !outgoing.to.is_empty() {
        builder = builder.to(mb_list(&outgoing.to));
    }
    if !outgoing.cc.is_empty() {
        builder = builder.cc(mb_list(&outgoing.cc));
    }
    if let Some(id) = &outgoing.in_reply_to {
        builder = builder.in_reply_to(id.clone());
    }
    if !outgoing.references.is_empty() {
        builder = builder.references(outgoing.references.clone());
    }
    for a in &outgoing.attachments {
        builder = builder.attachment(a.mime.clone(), a.name.clone(), a.data.clone());
    }
    let raw = builder.write_to_vec().map_err(|e| e.to_string())?;
    Ok((message_id, raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fuu_parser::mailparser::MailParserEngine;
    use fuu_parser::{MessageParser, ParseLimits};

    fn a(name: Option<&str>, email: &str) -> Address {
        Address { name: name.map(str::to_owned), email: email.into() }
    }

    #[test]
    fn parses_recipient_lists() {
        let r = parse_recipients(r#"a@example.com, 山田 太郎 <taro@example.jp>; "Smith, J" <j@example.org>、b@example.net"#).unwrap();
        assert_eq!(r.len(), 4);
        assert_eq!(r[1], Recipient { name: Some("山田 太郎".into()), email: "taro@example.jp".into() });
        assert_eq!(r[2].name.as_deref(), Some("Smith, J"));
        assert!(parse_recipients("").unwrap().is_empty());
        assert!(parse_recipients("not an address").is_err());
        assert!(parse_recipients("a@b, @x").is_err());
    }

    #[test]
    fn reply_and_forward_subjects() {
        assert_eq!(reply_subject("打ち合わせ"), "Re: 打ち合わせ");
        assert_eq!(reply_subject("Re: RE: re: x"), "Re: x");
        assert_eq!(reply_subject("Re[2]: x"), "Re: x");
        assert_eq!(reply_subject("AW: 返信: x"), "Re: x");
        assert_eq!(reply_subject("Fwd: x"), "Re: Fwd: x");
        assert_eq!(forward_subject("x"), "Fwd: x");
        assert_eq!(forward_subject("FW: x"), "FW: x");
    }

    #[test]
    fn reply_recipients_exclude_self_and_duplicates() {
        let h = Headers {
            from: vec![a(Some("Taro"), "taro@example.jp")],
            to: vec![a(None, "me@example.com"), a(None, "hanako@example.jp")],
            cc: vec![a(None, "TARO@example.jp"), a(None, "jiro@example.jp")],
            ..Headers::default()
        };
        let own = vec!["me@example.com".to_owned()];
        let (to, cc) = reply_recipients(&h, &own, false);
        assert_eq!(to.iter().map(|r| r.email.as_str()).collect::<Vec<_>>(), ["taro@example.jp"]);
        assert!(cc.is_empty());
        let (to, cc) = reply_recipients(&h, &own, true);
        assert_eq!(to.len(), 1);
        assert_eq!(cc.iter().map(|r| r.email.as_str()).collect::<Vec<_>>(), ["hanako@example.jp", "jiro@example.jp"]);
        let with_reply_to = Headers { reply_to: vec![a(None, "list@example.org")], ..h.clone() };
        assert_eq!(reply_recipients(&with_reply_to, &own, false).0[0].email, "list@example.org");
        let own_sent = Headers { from: vec![a(None, "me@example.com")], to: vec![a(None, "x@example.org")], ..Headers::default() };
        assert_eq!(reply_recipients(&own_sent, &own, false).0[0].email, "x@example.org");
    }

    #[test]
    fn original_header_is_outlook_style() {
        let h = Headers {
            from: vec![Address { name: Some("エンゲージ事務局".into()), email: "news@example.jp".into() }],
            to: vec![Address { name: None, email: "me@example.jp".into() }],
            subject: "【重要】お知らせ".into(),
            date: Some(1_763_463_780),
            ..Headers::default()
        };
        let text = original_header(&h);
        assert!(text.starts_with("差出人: \"エンゲージ事務局\" <news@example.jp>\n送信: "), "{text}");
        assert!(text.contains("曜日) ") && text.contains(" 年 ") && text.contains("宛先: me@example.jp\n件名: 【重要】お知らせ\n"), "{text}");
        assert!(!text.contains("CC:"));
    }

    #[test]
    fn quotes_with_attribution() {
        let q = quote("こんにちは\n> 前の引用\n", "Taro", "2026/11/18 10:00");
        assert_eq!(q, "2026/11/18 10:00、Taro さんは書きました:\n> こんにちは\n>> 前の引用\n");
    }

    #[test]
    fn builds_parseable_utf8_messages_without_bcc_header() {
        let outgoing = Outgoing {
            from_name: "山田 太郎".into(),
            from_email: "taro@example.jp".into(),
            to: vec![Recipient { name: Some("花子".into()), email: "hanako@example.jp".into() }],
            cc: vec![Recipient { name: None, email: "cc@example.jp".into() }],
            bcc: vec![Recipient { name: None, email: "secret@example.jp".into() }],
            subject: "Re: 日本語の件名".into(),
            body: "本文です。\n\n-- \n署名".into(),
            attachments: vec![OutgoingAttachment { name: "見積書.pdf".into(), mime: "application/pdf".into(), data: b"%PDF-1.4".to_vec() }],
            in_reply_to: Some("orig@example.jp".into()),
            references: vec!["root@example.jp".into(), "orig@example.jp".into()],
            html: false,
            html_body: None,
        };
        let (id, raw) = build(&outgoing).unwrap();
        assert!(id.ends_with("@example.jp"));
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.to_ascii_lowercase().contains("bcc:"), "BCC must not be in the headers");
        assert!(!text.contains("secret@example.jp"));
        let parsed = MailParserEngine.parse(&raw, &ParseLimits::default()).unwrap();
        assert_eq!(parsed.headers.subject, "Re: 日本語の件名");
        assert_eq!(parsed.headers.from[0].name.as_deref(), Some("山田 太郎"));
        assert_eq!(parsed.headers.cc[0].email, "cc@example.jp");
        assert_eq!(parsed.headers.in_reply_to, vec!["orig@example.jp".to_owned()]);
        assert_eq!(parsed.headers.references, vec!["root@example.jp".to_owned(), "orig@example.jp".to_owned()]);
        assert_eq!(parsed.attachments[0].file_name, "見積書.pdf");
        assert!(matches!(&parsed.body[0], fuu_model::BodyPart::Text(t) if t.contains("本文です")));
        assert_eq!(outgoing.envelope_recipients(), ["hanako@example.jp", "cc@example.jp", "secret@example.jp"]);
    }

    #[test]
    fn text_to_html_structure_and_escaping() {
        let html = text_to_html("こんにちは <b>太字ではない</b>\n詳細: https://example.org/a?b=1&c=2.\n\n2026/11/18、Taro さんは書きました:\n> 元の本文\n>> さらに前\n> 戻る\n\n-- \n山田 太郎\nhttps://example.jp");
        assert!(html.contains("&lt;b&gt;太字ではない&lt;/b&gt;"), "{html}");
        assert!(html.contains(r#"<a href="https://example.org/a?b=1&amp;c=2" "#), "trailing dot excluded: {html}");
        assert_eq!(html.matches("<blockquote").count(), 2, "{html}");
        assert_eq!(html.matches("</blockquote>").count(), 2, "{html}");
        assert!(html.contains("<div>元の本文</div>"));
        assert!(html.contains("color:#8a877f") && html.contains("山田 太郎"), "signature muted: {html}");
        assert!(!html.contains("<style"), "inline styles only");
    }

    #[test]
    fn html_messages_are_multipart_alternative() {
        let outgoing = Outgoing {
            from_email: "a@example.jp".into(),
            to: vec![Recipient { name: None, email: "b@example.jp".into() }],
            subject: "件名".into(),
            body: "本文 https://example.org\n\n-- \n署名".into(),
            html: true,
            ..Outgoing::default()
        };
        let (_, raw) = build(&outgoing).unwrap();
        let text = String::from_utf8_lossy(&raw).to_ascii_lowercase();
        assert!(text.contains("multipart/alternative"), "{text}");
        let parsed = MailParserEngine.parse(&raw, &ParseLimits::default()).unwrap();
        assert!(parsed.body.iter().any(|b| matches!(b, fuu_model::BodyPart::Html(h) if h.contains("<a href=\"https://example.org\""))));
        assert!(parsed.body.iter().any(|b| matches!(b, fuu_model::BodyPart::Text(t) if t.contains("本文"))));
        let plain = Outgoing { html: false, ..outgoing };
        let (_, raw) = build(&plain).unwrap();
        assert!(!String::from_utf8_lossy(&raw).to_ascii_lowercase().contains("text/html"));
    }

    #[test]
    fn refuses_messages_without_recipients() {
        let o = Outgoing { from_email: "a@example.jp".into(), ..Outgoing::default() };
        assert!(build(&o).is_err());
    }
}
