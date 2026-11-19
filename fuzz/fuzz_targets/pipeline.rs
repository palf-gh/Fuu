//! Raw message bytes through the same path the app uses: parse, then sanitise,
//! then the plain-text conversion used for quoting. Must never panic.
#![no_main]

use fuu_parser::mailparser::MailParserEngine;
use fuu_parser::{MessageParser, ParseLimits};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|raw: &[u8]| {
    let limits = ParseLimits { max_message_bytes: 1 << 20, ..ParseLimits::default() };
    if let Ok(message) = MailParserEngine.parse(raw, &limits) {
        let doc = fuu_sanitiser::sanitise(&message);
        let _ = fuu_sanitiser::html_to_text(&doc.html);
        for a in &message.attachments {
            let _ = fuu_sanitiser::filename::sanitise_file_name(&a.file_name);
        }
    }
});
