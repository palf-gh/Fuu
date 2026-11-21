//! Compile `lang/<lang>/LC_MESSAGES/fuu-app.po` (the same catalogues Slint
//! bundles) into sorted tables for `fuu_i18n::translate`.

use std::fmt::Write as _;
use std::path::Path;

fn unquote(s: &str) -> Option<String> {
    let s = s.trim();
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            other => out.push(other),
        }
    }
    Some(out)
}

/// (msgid, msgstr) pairs with a translation; fuzzy and empty entries are skipped.
fn parse(text: &str) -> Vec<(String, String)> {
    #[derive(PartialEq)]
    enum Field {
        None,
        Ctxt,
        Id,
        Str,
    }
    let mut entries = Vec::new();
    let (mut id, mut msgstr, mut has_ctxt, mut fuzzy) = (String::new(), String::new(), false, false);
    let mut field = Field::None;
    let mut flush = |id: &mut String, msgstr: &mut String, has_ctxt: &mut bool, fuzzy: &mut bool| {
        if !id.is_empty() && !msgstr.is_empty() && !*has_ctxt && !*fuzzy {
            entries.push((std::mem::take(id), std::mem::take(msgstr)));
        }
        id.clear();
        msgstr.clear();
        *has_ctxt = false;
        *fuzzy = false;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            flush(&mut id, &mut msgstr, &mut has_ctxt, &mut fuzzy);
            field = Field::None;
        } else if let Some(flags) = line.strip_prefix("#,") {
            fuzzy |= flags.contains("fuzzy");
        } else if line.starts_with('#') {
        } else if let Some(rest) = line.strip_prefix("msgctxt ") {
            has_ctxt = true;
            field = Field::Ctxt;
            let _ = rest;
        } else if let Some(rest) = line.strip_prefix("msgid ") {
            if field == Field::Str {
                flush(&mut id, &mut msgstr, &mut has_ctxt, &mut fuzzy);
            }
            id = unquote(rest).unwrap_or_default();
            field = Field::Id;
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            msgstr = unquote(rest).unwrap_or_default();
            field = Field::Str;
        } else if line.starts_with('"') {
            let more = unquote(line).unwrap_or_default();
            match field {
                Field::Id => id.push_str(&more),
                Field::Str => msgstr.push_str(&more),
                _ => {}
            }
        }
    }
    flush(&mut id, &mut msgstr, &mut has_ctxt, &mut fuzzy);
    entries
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lang");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut languages: Vec<(String, Vec<(String, String)>)> = Vec::new();
    if let Ok(dirs) = std::fs::read_dir(&root) {
        for dir in dirs.flatten() {
            let po = dir.path().join("LC_MESSAGES").join("fuu-app.po");
            println!("cargo:rerun-if-changed={}", po.display());
            let Ok(text) = std::fs::read_to_string(&po) else { continue };
            let mut entries = parse(&text);
            entries.sort();
            entries.dedup_by(|a, b| a.0 == b.0);
            languages.push((dir.file_name().to_string_lossy().into_owned(), entries));
        }
    }
    languages.sort();
    let mut out = String::from("pub(crate) static CATALOGUES: &[(&str, &[(&str, &str)])] = &[\n");
    for (lang, entries) in &languages {
        let _ = writeln!(out, "    ({lang:?}, &[");
        for (id, s) in entries {
            let _ = writeln!(out, "        ({id:?}, {s:?}),");
        }
        out.push_str("    ]),\n");
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("catalogues.rs");
    std::fs::write(dest, out).unwrap();
}
