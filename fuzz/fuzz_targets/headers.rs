//! Header decoding (RFC 2047 encoded words, legacy charsets) on arbitrary bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|raw: &[u8]| {
    let _ = fuu_parser::headers::decode_header(raw);
    let _ = fuu_parser::headers::decode_raw(raw);
});
