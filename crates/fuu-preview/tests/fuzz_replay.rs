//! Stable stand-in for the `preview` cargo-fuzz target: every seed in
//! `fuzz/corpus/preview` plus deterministic mutations goes through conversion
//! and sanitising. Any panic fails the test; so does a `<script>` tag or a
//! `javascript:` URL inside a tag of the sanitised document.

use std::path::Path;

/// Mutations per seed; PDF pages are rasterised, so PDFs get fewer.
const MUTATIONS: usize = 80;
const PDF_MUTATIONS: usize = 15;

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

const TOKENS: &[&[u8]] = &[
    b"<", b">", b"\"", b"javascript:", b"TargetMode=\"External\"", b"r:embed=\"", b"/Length 999999",
    b"obj", b"endobj", b"stream", b"\x00", b"\xff\xd8\xff", b"PK\x03\x04", b"IHDR",
];

fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut out = seed.to_vec();
    for _ in 0..1 + rng.below(3) {
        let at = rng.below(out.len() + 1);
        match rng.below(4) {
            0 if !out.is_empty() => {
                let i = rng.below(out.len());
                out[i] ^= 1 << rng.below(8);
            }
            1 => out.truncate(at.max(out.len() / 2)),
            2 => {
                let t = TOKENS[rng.below(TOKENS.len())];
                out.splice(at..at, t.iter().copied());
            }
            _ => out.insert(at.min(out.len()), rng.next() as u8),
        }
    }
    out
}

/// The markup tags of a serialised document (text is escaped and has no `<`).
fn tags(html: &str) -> impl Iterator<Item = &str> {
    html.match_indices('<').filter_map(move |(i, _)| html[i..].find('>').map(|j| &html[i..=i + j]))
}

fn check(name: &str, data: &[u8]) {
    if let Ok(message) = fuu_preview::preview(name, "", data) {
        let doc = fuu_sanitiser::sanitise(&message);
        for tag in tags(&doc.html) {
            let tag: String = tag.to_ascii_lowercase().chars().filter(|c| !c.is_ascii_whitespace()).collect();
            assert!(!tag.starts_with("<script"), "{name}: script survived");
            assert!(!tag.contains("javascript:"), "{name}: javascript: URL survived in {tag}");
        }
    }
}

#[test]
fn previews_never_panic_or_leak_script() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/preview");
    let mut seeds: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).collect();
    seeds.sort();
    assert!(seeds.len() >= 5, "seeds in {}", dir.display());
    for path in seeds {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let seed = std::fs::read(&path).unwrap();
        check(&name, &seed);
        let mut rng = Rng(0x51_7c_c1_b7_27_22_0a_95 ^ seed.len() as u64);
        let rounds = if name.ends_with(".pdf") { PDF_MUTATIONS } else { MUTATIONS };
        for i in 0..rounds {
            let input = mutate(&seed, &mut rng);
            if std::panic::catch_unwind(|| check(&name, &input)).is_err() {
                let dump = std::env::temp_dir().join(format!("fuu-fuzz-preview-{name}-{i}.bin"));
                let _ = std::fs::write(&dump, &input);
                panic!("{name} mutation {i} panicked; input saved to {}", dump.display());
            }
        }
    }
}
