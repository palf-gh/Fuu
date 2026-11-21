//! UI language and string translation, shared by every crate that shows text.
//!
//! Source strings are English. Translations are the gettext catalogues in
//! `lang/<lang>/LC_MESSAGES/fuu-app.po` at the repository root (the same files
//! Slint bundles for `@tr()`), compiled in by `build.rs`. Placeholders follow
//! Slint: `{}` takes the next argument, `{0}`…`{9}` a given one, `{{`/`}}` are
//! literal braces.
//!
//! ```
//! use fuu_i18n::tr;
//! let host = "imap.example.com";
//! let text = tr!("Connecting to {}…", host);
//! assert!(text.contains(host));
//! ```

use std::fmt::Display;
use std::sync::atomic::{AtomicUsize, Ordering};

include!(concat!(env!("OUT_DIR"), "/catalogues.rs"));

/// Languages with a catalogue, besides English (the source).
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("ja", "日本語"),
    ("zh-Hans", "简体中文"),
    ("zh-Hant", "繁體中文"),
    ("ko", "한국어"),
    ("pt-BR", "Português (Brasil)"),
    ("es-ES", "Español (España)"),
    ("es-419", "Español (Latinoamérica)"),
    ("de", "Deutsch"),
    ("fr", "Français"),
    ("id", "Bahasa Indonesia"),
];

const ENGLISH: usize = usize::MAX;
static CURRENT: AtomicUsize = AtomicUsize::new(ENGLISH);

/// The catalogue folder for an OS locale such as `ja-JP`, `zh-TW`, `es-MX`
/// or `pt_PT.UTF-8`; `"en"` when there is none.
pub fn language_for_locale(locale: &str) -> &'static str {
    let locale = locale.split(['.', '@']).next().unwrap_or_default().replace('_', "-");
    let mut parts = locale.split('-');
    let base = parts.next().unwrap_or_default().to_ascii_lowercase();
    let rest: Vec<String> = parts.map(|p| p.to_ascii_lowercase()).collect();
    let has = |s: &str| rest.iter().any(|p| p == s);
    match base.as_str() {
        "zh" if has("hant") || has("tw") || has("hk") || has("mo") => "zh-Hant",
        "zh" => "zh-Hans",
        "es" if has("es") => "es-ES",
        "es" => "es-419",
        "pt" => "pt-BR",
        "ja" => "ja",
        "ko" => "ko",
        "de" => "de",
        "fr" => "fr",
        "id" | "in" => "id",
        _ => "en",
    }
}

/// Use `language` (a code from [`LANGUAGES`]); unknown codes select English.
/// Returns the code now in use.
pub fn set_language(language: &str) -> &'static str {
    match CATALOGUES.iter().position(|(code, _)| *code == language) {
        Some(i) => {
            CURRENT.store(i, Ordering::Relaxed);
            CATALOGUES[i].0
        }
        None => {
            CURRENT.store(ENGLISH, Ordering::Relaxed);
            "en"
        }
    }
}

/// The language in use.
pub fn language() -> &'static str {
    CATALOGUES.get(CURRENT.load(Ordering::Relaxed)).map_or("en", |(code, _)| code)
}

/// The translation of an English source string (itself when untranslated).
pub fn translate(source: &'static str) -> &'static str {
    let Some((_, entries)) = CATALOGUES.get(CURRENT.load(Ordering::Relaxed)) else { return source };
    entries.binary_search_by(|(id, _)| (*id).cmp(source)).map_or(source, |i| entries[i].1)
}

/// Fill `{}` / `{0}`… placeholders in a translated template.
pub fn format(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0;
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut index = String::new();
                while let Some(&d) = chars.peek() {
                    chars.next();
                    if d == '}' {
                        break;
                    }
                    index.push(d);
                }
                let i = if index.is_empty() {
                    next += 1;
                    next - 1
                } else {
                    index.parse().unwrap_or(usize::MAX)
                };
                if let Some(arg) = args.get(i) {
                    use std::fmt::Write;
                    let _ = write!(out, "{arg}");
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// `tr!("English")` → the translated `&'static str`;
/// `tr!("English {}", value)` → a `String` with the placeholders filled.
#[macro_export]
macro_rules! tr {
    ($source:literal) => {
        $crate::translate($source)
    };
    ($source:literal, $($arg:expr),+ $(,)?) => {
        $crate::format($crate::translate($source), &[$(&$arg as &dyn ::std::fmt::Display),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_map_to_catalogues() {
        assert_eq!(language_for_locale("ja-JP"), "ja");
        assert_eq!(language_for_locale("zh-CN"), "zh-Hans");
        assert_eq!(language_for_locale("zh-TW"), "zh-Hant");
        assert_eq!(language_for_locale("zh_HK.UTF-8"), "zh-Hant");
        assert_eq!(language_for_locale("zh-Hant-TW"), "zh-Hant");
        assert_eq!(language_for_locale("es-ES"), "es-ES");
        assert_eq!(language_for_locale("es-MX"), "es-419");
        assert_eq!(language_for_locale("pt-PT"), "pt-BR");
        assert_eq!(language_for_locale("en-GB"), "en");
        assert_eq!(language_for_locale("nl-NL"), "en");
    }

    #[test]
    fn formats_placeholders() {
        assert_eq!(format("{} of {}", &[&1, &2]), "1 of 2");
        assert_eq!(format("{1} before {0}", &[&"a", &"b"]), "b before a");
        assert_eq!(format("{{literal}} {}", &[&3]), "{literal} 3");
        assert_eq!(format("missing {}", &[]), "missing ");
    }

    #[test]
    fn falls_back_to_english() {
        set_language("xx");
        assert_eq!(language(), "en");
        assert_eq!(translate("Not in any catalogue"), "Not in any catalogue");
        assert_eq!(tr!("{} new", 3), "3 new");
    }

    #[test]
    fn every_catalogue_is_sorted() {
        for (code, entries) in CATALOGUES {
            assert!(entries.windows(2).all(|w| w[0].0 < w[1].0), "{code} not sorted");
            assert!(LANGUAGES.iter().any(|(c, _)| c == code), "{code} not listed");
        }
    }
}
