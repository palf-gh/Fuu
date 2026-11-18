//! Lenient header decoding (RFC 2047 encoded words and raw 8-bit text).
//!
//! Real-world mail breaks the RFC in predictable ways, so this decoder:
//! - accepts encoded words longer than the 75-character limit;
//! - joins adjacent encoded words with the same charset and encoding at the byte
//!   level before converting, so a multibyte character split across two words
//!   decodes correctly;
//! - drops the whitespace between adjacent encoded words (RFC 2047 §6.2) and
//!   unfolds line breaks;
//! - tolerates missing base64 padding and stray whitespace;
//! - decodes unencoded 8-bit text as UTF-8, ISO-2022-JP or Shift_JIS.

use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use base64::Engine as _;
use encoding_rs::Encoding;

const LENIENT_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent).with_decode_allow_trailing_bits(true),
);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(Vec<u8>),
    Encoded { charset: String, bytes: Vec<u8> },
}

/// Resolve a MIME charset label (with optional RFC 2231 `*lang` suffix).
fn encoding_for(label: &str) -> Option<&'static Encoding> {
    let label = label.split('*').next().unwrap_or(label).trim().to_ascii_lowercase();
    let label = match label.as_str() {
        "cp932" | "ms932" | "windows-31j" | "x-sjis" | "sjis" => "shift_jis",
        "utf8" => "utf-8",
        other => other,
    };
    Encoding::for_label(label.as_bytes())
}

/// Decode raw (non-encoded-word) bytes.
pub fn decode_raw(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        if !text.contains('\u{1b}') {
            return text.to_owned();
        }
    }
    if bytes.contains(&0x1b) {
        let (text, _, had_errors) = encoding_rs::ISO_2022_JP.decode(bytes);
        if !had_errors {
            return text.into_owned();
        }
    }
    let (text, _, had_errors) = encoding_rs::SHIFT_JIS.decode(bytes);
    if !had_errors {
        return text.into_owned();
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn decode_q(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            b'_' => out.push(b' '),
            b'=' if i + 2 < text.len() => {
                let hex = std::str::from_utf8(&text[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'='),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    out
}

/// Try to read an encoded word `=?charset?enc?text?=` at `input[start..]`.
/// Returns the piece and the index just past it.
fn encoded_word_at(input: &[u8], start: usize) -> Option<(Piece, usize)> {
    let rest = input.get(start..)?;
    if !rest.starts_with(b"=?") {
        return None;
    }
    let after = &rest[2..];
    let q1 = after.iter().position(|&b| b == b'?')?;
    let charset = std::str::from_utf8(&after[..q1]).ok()?;
    if charset.is_empty() || charset.bytes().any(|b| b.is_ascii_whitespace()) {
        return None;
    }
    let enc = *after.get(q1 + 1)?;
    if after.get(q1 + 2) != Some(&b'?') {
        return None;
    }
    let payload_start = q1 + 3;
    let payload_len = after[payload_start..].windows(2).position(|w| w == b"?=")?;
    let payload = &after[payload_start..payload_start + payload_len];
    if payload.iter().any(|&b| b == b'\r' || b == b'\n') {
        return None;
    }
    let bytes = match enc.to_ascii_uppercase() {
        b'B' => {
            let cleaned: Vec<u8> = payload.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
            LENIENT_BASE64.decode(&cleaned).ok()?
        }
        b'Q' => decode_q(payload),
        _ => return None,
    };
    let end = start + 2 + payload_start + payload_len + 2;
    Some((Piece::Encoded { charset: charset.to_owned(), bytes }, end))
}

fn tokenize(input: &[u8]) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut text = Vec::new();
    let mut i = 0;
    while i < input.len() {
        if let Some((piece, end)) = encoded_word_at(input, i) {
            // Whitespace between two encoded words is not displayed.
            let previous_is_encoded = matches!(pieces.last(), Some(Piece::Encoded { .. }));
            if !text.is_empty() {
                if !(previous_is_encoded && text.iter().all(|b: &u8| b.is_ascii_whitespace())) {
                    pieces.push(Piece::Text(std::mem::take(&mut text)));
                } else {
                    text.clear();
                }
            }
            // Join with the previous encoded word when charset matches.
            match (pieces.last_mut(), piece) {
                (Some(Piece::Encoded { charset: c1, bytes: b1 }), Piece::Encoded { charset: c2, bytes: b2 })
                    if c1.eq_ignore_ascii_case(&c2) =>
                {
                    b1.extend_from_slice(&b2);
                }
                (_, piece) => pieces.push(piece),
            }
            i = end;
        } else {
            text.push(input[i]);
            i += 1;
        }
    }
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    pieces
}

/// Decode a header value to display text.
pub fn decode_header(raw: &[u8]) -> String {
    // Unfold: CRLF followed by whitespace is a single space.
    let mut unfolded = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'\r' | b'\n' => {
                while i < raw.len() && matches!(raw[i], b'\r' | b'\n') {
                    i += 1;
                }
                if !unfolded.last().is_some_and(|b: &u8| b.is_ascii_whitespace()) {
                    unfolded.push(b' ');
                }
                continue;
            }
            b => unfolded.push(b),
        }
        i += 1;
    }
    let mut out = String::new();
    for piece in tokenize(&unfolded) {
        match piece {
            Piece::Text(bytes) => out.push_str(&decode_raw(&bytes)),
            Piece::Encoded { charset, bytes } => match encoding_for(&charset) {
                Some(encoding) => out.push_str(&encoding.decode_without_bom_handling(&bytes).0),
                None => out.push_str(&String::from_utf8_lossy(&bytes)),
            },
        }
    }
    let collapsed: Vec<&str> = out.split_whitespace().collect();
    collapsed.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(s: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(s)
    }

    #[test]
    fn basic_words() {
        assert_eq!(decode_header(b"=?UTF-8?B?44GT44KT44Gr44Gh44Gv?="), "こんにちは");
        assert_eq!(decode_header(b"=?ISO-2022-JP?B?GyRCJUYlOSVIGyhC?="), "テスト");
        assert_eq!(decode_header(b"Re: =?UTF-8?Q?caf=C3=A9?= menu"), "Re: café menu");
        assert_eq!(decode_header(b"=?utf-8?q?a_b?="), "a b");
        assert_eq!(decode_header(b"plain\r\n folded"), "plain folded");
    }

    #[test]
    fn over_long_words_and_folding() {
        // One encoded word well past the 75-character limit, then a second word on
        // a folded line, as many mailers produce.
        let first = "Re: 【お知らせ】オンライン講座の開講について、詳細とスケジュール";
        let raw = format!("=?UTF-8?B?{}?=\r\n =?UTF-8?B?{}?=", b64(first.as_bytes()), b64("のご案内".as_bytes()));
        assert!(raw.split("?=").next().unwrap().len() > 75);
        assert_eq!(decode_header(raw.as_bytes()), format!("{first}のご案内"));
    }

    #[test]
    fn multibyte_split_across_words() {
        let bytes = "日本語の件名".as_bytes();
        let (a, b) = bytes.split_at(4); // splits inside 本 (3-byte UTF-8)
        let raw = format!("=?UTF-8?B?{}?= =?UTF-8?B?{}?=", b64(a), b64(b));
        assert_eq!(decode_header(raw.as_bytes()), "日本語の件名");
    }

    #[test]
    fn whitespace_kept_between_text_and_words() {
        let raw = format!("Fwd: =?UTF-8?B?{}?= (2)", b64("資料".as_bytes()));
        assert_eq!(decode_header(raw.as_bytes()), "Fwd: 資料 (2)");
    }

    #[test]
    fn missing_padding_and_cp932_label() {
        let raw = format!("=?UTF-8?B?{}?=", b64("テスト".as_bytes()).trim_end_matches('='));
        assert_eq!(decode_header(raw.as_bytes()), "テスト");
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("件名");
        assert_eq!(decode_header(format!("=?CP932?B?{}?=", b64(&sjis)).as_bytes()), "件名");
    }

    #[test]
    fn raw_8bit_text() {
        assert_eq!(decode_header("生のUTF-8".as_bytes()), "生のUTF-8");
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("シフトJIS");
        assert_eq!(decode_header(&sjis), "シフトJIS");
        let (jis, _, _) = encoding_rs::ISO_2022_JP.encode("ジス");
        assert_eq!(decode_header(&jis), "ジス");
    }

    #[test]
    fn malformed_words_stay_literal() {
        assert_eq!(decode_header(b"=?UTF-8?X?abc?="), "=?UTF-8?X?abc?=");
        assert_eq!(decode_header(b"=?UTF-8?B?unterminated"), "=?UTF-8?B?unterminated");
        assert_eq!(decode_header(b"a =? b"), "a =? b");
    }
}
