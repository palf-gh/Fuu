//! Attachment previews: the first byte picks the format (as a file name would),
//! the rest is the file. Conversion plus sanitising must never panic, and no
//! script may survive.
#![no_main]

use libfuzzer_sys::fuzz_target;

const NAMES: &[&str] = &["a.png", "a.jpg", "a.gif", "a.pdf", "a.docx", "a.xlsx", "a.xls", "a.ods", "a.pptx"];

fuzz_target!(|input: &[u8]| {
    let Some((&pick, data)) = input.split_first() else { return };
    let name = NAMES[pick as usize % NAMES.len()];
    if let Ok(message) = fuu_preview::preview(name, "", data) {
        let doc = fuu_sanitiser::sanitise(&message);
        assert!(!doc.html.to_ascii_lowercase().contains("<script"));
    }
});
