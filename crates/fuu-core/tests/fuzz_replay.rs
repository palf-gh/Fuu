//! Stable stand-in for the cargo-fuzz targets in `fuzz/`: every seed in
//! `fuzz/corpus/*`, plus a fixed set of deterministic mutations of it, goes
//! through the same code the fuzz targets exercise. Any panic fails the test.
//! Crash reproducers found by cargo-fuzz belong in the corpus, so they are
//! replayed here on every `cargo test`.

use std::path::{Path, PathBuf};

use fuu_parser::mailparser::MailParserEngine;
use fuu_parser::{MessageParser, ParseLimits};

/// Mutations per seed. Kept small so the debug test stays quick.
const MUTATIONS: usize = 500;

fn corpus(target: &str) -> Vec<(PathBuf, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus").join(target);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no seeds in {}", dir.display());
    files.into_iter().map(|p| (p.clone(), std::fs::read(&p).unwrap())).collect()
}

/// xorshift64*: deterministic, so a failure reproduces exactly.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

/// Fragments that steer mutations towards the parsers' tricky paths.
const TOKENS: &[&[u8]] = &[
    b"\r\n--", b"boundary=\"", b"=?UTF-8?B?", b"=?ISO-2022-JP?Q?", b"?=", b"\x1b$B", b"\x1b(B",
    b"Content-Transfer-Encoding: base64\r\n", b"Content-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\n",
    b"<", b">", b"<style>", b"url(", b"@import", b"cid:", b"&#", b"\0", b"\xff\xfe", b"\xe2\x80\xae",
    b"=\r\n", b"%", b"\"", b"\\", b";", b"{", b"}",
];

fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut out = seed.to_vec();
    for _ in 0..1 + rng.below(4) {
        let at = rng.below(out.len() + 1);
        match rng.below(6) {
            0 if !out.is_empty() => {
                let i = rng.below(out.len());
                out[i] ^= 1 << rng.below(8);
            }
            1 => out.truncate(at),
            2 => {
                let token = TOKENS[rng.below(TOKENS.len())];
                out.splice(at..at, token.iter().copied());
            }
            3 if !out.is_empty() => {
                let end = (at + rng.below(64)).min(out.len());
                out.drain(at.min(end)..end);
            }
            4 if !out.is_empty() => {
                // Duplicate a slice (repeated parts, nested boundaries).
                let start = rng.below(out.len());
                let end = (start + rng.below(256)).min(out.len());
                let copy = out[start..end].to_vec();
                let at = at.min(out.len());
                out.splice(at..at, copy);
            }
            _ => out.insert(at.min(out.len()), rng.next() as u8),
        }
    }
    out
}

fn each_input(target: &str, check: impl Fn(&[u8])) {
    for (path, seed) in corpus(target) {
        check(&seed);
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ seed.len() as u64);
        for i in 0..MUTATIONS {
            let input = mutate(&seed, &mut rng);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&input)));
            if outcome.is_err() {
                let dump = std::env::temp_dir().join(format!("fuu-fuzz-{target}-{i}.bin"));
                let _ = std::fs::write(&dump, &input);
                panic!("{} mutation {i} panicked; input saved to {}", path.display(), dump.display());
            }
        }
    }
}

/// The markup tags of a serialised document (text is escaped and has no `<`).
fn tags(html: &str) -> impl Iterator<Item = &str> {
    html.match_indices('<').filter_map(move |(i, _)| html[i..].find('>').map(|j| &html[i..=i + j]))
}

#[test]
fn pipeline_never_panics() {
    let limits = ParseLimits { max_message_bytes: 1 << 20, ..ParseLimits::default() };
    each_input("pipeline", |raw| {
        if let Ok(message) = MailParserEngine.parse(raw, &limits) {
            let doc = fuu_sanitiser::sanitise(&message);
            let _ = fuu_sanitiser::html_to_text(&doc.html);
            for a in &message.attachments {
                let _ = fuu_sanitiser::filename::sanitise_file_name(&a.file_name);
            }
            // The sanitised document never carries script: no <script> element
            // and no javascript: URL inside a tag. Text is escaped (`&lt;`), so
            // only real tags start with `<`.
            for tag in tags(&doc.html) {
                let tag: String = tag.to_ascii_lowercase().chars().filter(|c| !c.is_ascii_whitespace()).collect();
                assert!(!tag.starts_with("<script"), "script survived: {}", doc.html);
                assert!(!tag.contains("javascript:"), "javascript: URL survived in {tag}");
            }
        }
    });
}

#[test]
fn css_filter_never_panics() {
    each_input("css", |raw| {
        let text = String::from_utf8_lossy(raw);
        let inline = fuu_sanitiser::css::sanitise_declarations(&text);
        let sheet = fuu_sanitiser::css::sanitise_stylesheet(&text);
        for out in [inline, sheet] {
            assert!(!out.to_ascii_lowercase().contains("url("), "url() survived: {out}");
        }
    });
}

#[test]
fn header_decoding_never_panics() {
    each_input("headers", |raw| {
        let _ = fuu_parser::headers::decode_header(raw);
        let _ = fuu_parser::headers::decode_raw(raw);
    });
}
