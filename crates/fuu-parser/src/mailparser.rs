//! Default parsing engine, backed by the `mail-parser` crate. Only Fuu's own
//! [`ParsedMessage`] leaves this module.

use std::borrow::Cow;

use fuu_model::{Address, Attachment, BodyPart, Headers, InlineResource, ParsedMessage};
use mail_parser::{HeaderValue, Message, MessageParser as MpParser, MimeHeaders, PartType};

use crate::{headers, MessageParser, ParseError, ParseLimits};
use fuu_i18n::tr;

#[derive(Debug, Default, Clone, Copy)]
pub struct MailParserEngine;

impl MessageParser for MailParserEngine {
    fn parse(&self, raw: &[u8], limits: &ParseLimits) -> Result<ParsedMessage, ParseError> {
        if raw.len() > limits.max_message_bytes {
            return Err(ParseError::TooLarge);
        }
        // A panic inside the parser must never take the UI down.
        std::panic::catch_unwind(|| parse_inner(raw, limits))
            .unwrap_or_else(|_| Err(ParseError::Malformed("parser panicked".into())))
    }
}

/// Bidirectional-control characters are stripped from everything shown as text.
pub fn strip_bidi(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')).collect()
}

fn addresses(list: Option<&mail_parser::Address<'_>>) -> Vec<Address> {
    let Some(list) = list else { return Vec::new() };
    list.iter()
        .filter_map(|a| {
            let email = a.address.as_deref()?.trim().to_owned();
            let name = a.name.as_deref().map(|n| strip_bidi(n.trim())).filter(|n| !n.is_empty());
            Some(Address { name, email: strip_bidi(&email) })
        })
        .collect()
}

fn ids(value: &HeaderValue<'_>) -> Vec<String> {
    match value {
        HeaderValue::Text(t) => vec![t.to_string()],
        HeaderValue::TextList(list) => list.iter().map(|t| t.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// Nesting depth of attached (message/rfc822) messages.
fn message_depth(message: &Message<'_>, limit: usize, depth: usize) -> Result<(), ParseError> {
    if depth > limit {
        return Err(ParseError::TooDeep);
    }
    for part in &message.parts {
        if let PartType::Message(inner) = &part.body {
            message_depth(inner, limit, depth + 1)?;
        }
    }
    Ok(())
}

fn mime_of(part: &mail_parser::MessagePart<'_>) -> String {
    match part.content_type() {
        Some(ct) => match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub).to_ascii_lowercase(),
            None => ct.ctype().to_ascii_lowercase(),
        },
        None => match part.body {
            PartType::Html(_) => "text/html".into(),
            PartType::Text(_) => "text/plain".into(),
            _ => "application/octet-stream".into(),
        },
    }
}

fn parse_inner(raw: &[u8], limits: &ParseLimits) -> Result<ParsedMessage, ParseError> {
    let message = MpParser::default().parse(raw).ok_or_else(|| ParseError::Malformed("not an RFC 5322 message".into()))?;
    if message.parts.len() > limits.max_parts {
        return Err(ParseError::TooManyParts);
    }
    message_depth(&message, limits.max_mime_depth, 0)?;
    let decoded: usize = message.parts.iter().map(|p| p.contents().len()).sum();
    if decoded > limits.max_decoded_bytes {
        return Err(ParseError::TooLarge);
    }

    // Prefer our lenient decoder when the library leaves encoded words behind.
    let subject = match message.subject() {
        Some(s) if s.contains("=?") && s.contains("?=") => headers::decode_header(s.as_bytes()),
        Some(s) => s.to_owned(),
        None => String::new(),
    };
    let headers = Headers {
        message_id: message.message_id().map(str::to_owned),
        in_reply_to: ids(message.in_reply_to()),
        references: ids(message.references()),
        subject: strip_bidi(subject.trim()),
        from: addresses(message.from()),
        reply_to: addresses(message.reply_to()),
        to: addresses(message.to()),
        cc: addresses(message.cc()),
        date: message.date().map(|d| d.to_timestamp()),
    };

    let mut body = Vec::new();
    for &id in &message.html_body {
        if let Some(PartType::Html(html)) = message.parts.get(id as usize).map(|p| &p.body) {
            body.push(BodyPart::Html(html.to_string()));
        }
    }
    for &id in &message.text_body {
        if let Some(PartType::Text(text)) = message.parts.get(id as usize).map(|p| &p.body) {
            body.push(BodyPart::Text(text.to_string()));
        }
    }

    let mut inline = Vec::new();
    for part in &message.parts {
        let mime = mime_of(part);
        if let (Some(cid), true) = (part.content_id(), mime.starts_with("image/")) {
            let cid = cid.trim().trim_start_matches('<').trim_end_matches('>').to_owned();
            if !cid.is_empty() {
                inline.push(InlineResource { content_id: cid, mime, data: part.contents().to_vec() });
            }
        }
    }

    let mut attachments = Vec::new();
    for (n, &id) in message.attachments.iter().enumerate() {
        let Some(part) = message.parts.get(id as usize) else { continue };
        let mime = mime_of(part);
        // Inline images referenced by cid are shown in the body, not listed.
        let is_inline_image = part.content_id().is_some()
            && mime.starts_with("image/")
            && part.content_disposition().map(|d| !d.ctype().eq_ignore_ascii_case("attachment")).unwrap_or(true);
        if is_inline_image {
            continue;
        }
        let name: Cow<'_, str> = match part.attachment_name() {
            Some(name) => Cow::Owned(strip_bidi(name)),
            None if matches!(part.body, PartType::Message(_)) => Cow::Owned(tr!("Forwarded message {0}.eml", n + 1)),
            None => Cow::Owned(tr!("Attachment {0}", n + 1)),
        };
        attachments.push(Attachment { file_name: name.into_owned(), mime, size: part.contents().len() as u64, part_index: id as usize });
    }

    Ok(ParsedMessage { headers, body, attachments, inline })
}

/// Bytes of one attachment, by the `part_index` reported in [`Attachment`].
pub fn attachment_bytes(raw: &[u8], part_index: usize) -> Option<Vec<u8>> {
    let message = MpParser::default().parse(raw)?;
    let part = message.parts.get(part_index)?;
    match &part.body {
        PartType::Message(_) => raw.get(part.offset_body as usize..part.offset_end as usize).map(<[u8]>::to_vec),
        _ => Some(part.contents().to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Result<ParsedMessage, ParseError> {
        MailParserEngine.parse(raw.replace('\n', "\r\n").as_bytes(), &ParseLimits::default())
    }

    const MULTIPART: &str = "From: =?UTF-8?B?5bGx55SwIOWkqumDjg==?= <taro@example.jp>
To: a@example.com, B <b@example.com>
Cc: c@example.com
Subject: =?ISO-2022-JP?B?GyRCJUYlOSVIGyhC?=
Message-ID: <m1@example.jp>
In-Reply-To: <m0@example.jp>
Date: Tue, 17 Nov 2026 10:00:00 +0900
MIME-Version: 1.0
Content-Type: multipart/mixed; boundary=\"mix\"

--mix
Content-Type: multipart/related; boundary=\"rel\"

--rel
Content-Type: multipart/alternative; boundary=\"alt\"

--alt
Content-Type: text/plain; charset=ISO-2022-JP
Content-Transfer-Encoding: 7bit

\u{1b}$B$3$s$K$A$O\u{1b}(B
--alt
Content-Type: text/html; charset=UTF-8

<p>hello <img src=\"cid:logo@x\"></p>
--alt--
--rel
Content-Type: image/png
Content-ID: <logo@x>
Content-Transfer-Encoding: base64

iVBORw0KGgo=
--rel--
--mix
Content-Type: application/pdf; name=\"=?UTF-8?B?6KaL56mN5pu4LnBkZg==?=\"
Content-Disposition: attachment; filename=\"=?UTF-8?B?6KaL56mN5pu4LnBkZg==?=\"
Content-Transfer-Encoding: base64

JVBERi0=
--mix--
";

    #[test]
    fn parses_headers_bodies_inline_and_attachments() {
        let m = parse(MULTIPART).unwrap();
        assert_eq!(m.headers.subject, "テスト");
        assert_eq!(m.headers.from[0].name.as_deref(), Some("山田 太郎"));
        assert_eq!(m.headers.to.len(), 2);
        assert_eq!(m.headers.cc[0].email, "c@example.com");
        assert_eq!(m.headers.message_id.as_deref(), Some("m1@example.jp"));
        assert_eq!(m.headers.in_reply_to, vec!["m0@example.jp".to_owned()]);
        assert!(m.headers.date.is_some());
        assert!(m.body.iter().any(|b| matches!(b, BodyPart::Text(t) if t.contains("こんにちは"))), "{:?}", m.body);
        assert!(m.body.iter().any(|b| matches!(b, BodyPart::Html(h) if h.contains("cid:logo@x"))));
        assert_eq!(m.inline.len(), 1);
        assert_eq!(m.inline[0].content_id, "logo@x");
        assert_eq!(m.inline[0].mime, "image/png");
        assert_eq!(m.attachments.len(), 1, "{:?}", m.attachments);
        assert_eq!(m.attachments[0].file_name, "見積書.pdf");
        assert_eq!(m.attachments[0].mime, "application/pdf");
        let bytes = attachment_bytes(MULTIPART.replace('\n', "\r\n").as_bytes(), m.attachments[0].part_index).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn plain_shift_jis_message() {
        let (body, _, _) = encoding_rs::SHIFT_JIS.encode("本文です");
        let mut raw = b"From: a@example.jp\r\nSubject: s\r\nContent-Type: text/plain; charset=Shift_JIS\r\n\r\n".to_vec();
        raw.extend_from_slice(&body);
        let m = MailParserEngine.parse(&raw, &ParseLimits::default()).unwrap();
        assert_eq!(m.body, vec![BodyPart::Text("本文です".into())]);
        assert!(m.attachments.is_empty());
    }

    #[test]
    fn enforces_limits() {
        let tiny = ParseLimits { max_message_bytes: 10, ..ParseLimits::default() };
        assert_eq!(MailParserEngine.parse(MULTIPART.as_bytes(), &tiny), Err(ParseError::TooLarge));
        let few_parts = ParseLimits { max_parts: 2, ..ParseLimits::default() };
        assert_eq!(MailParserEngine.parse(MULTIPART.as_bytes(), &few_parts), Err(ParseError::TooManyParts));
        let nested = "Content-Type: message/rfc822\r\n\r\nContent-Type: message/rfc822\r\n\r\nSubject: deep\r\n\r\nx";
        let shallow = ParseLimits { max_mime_depth: 0, ..ParseLimits::default() };
        assert_eq!(MailParserEngine.parse(nested.as_bytes(), &shallow), Err(ParseError::TooDeep));
    }

    #[test]
    fn strips_bidi_from_names_and_subjects() {
        let m = parse("From: \u{202E}evil <e@example.com>\nSubject: inv\u{202E}fdp.exe\n\nx").unwrap();
        assert_eq!(m.headers.subject, "invfdp.exe");
        assert_eq!(m.headers.from[0].name.as_deref(), Some("evil"));
    }
}
