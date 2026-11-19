//! The CSS filter on arbitrary text: inline declarations and whole stylesheets.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = fuu_sanitiser::css::sanitise_declarations(text);
    let _ = fuu_sanitiser::css::sanitise_stylesheet(text);
});
