//! Reading Office Open XML packages (zip) with hard limits, plus the
//! relationship files that tie their parts together.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::PreviewError;

/// Zip entries allowed in a package.
const MAX_ENTRIES: usize = 4000;
/// Bytes read from one entry (enforced on the data, not the declared size).
const MAX_ENTRY_BYTES: u64 = 48 * 1024 * 1024;
/// Bytes read from the whole package.
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) struct Package {
    zip: zip::ZipArchive<Cursor<Vec<u8>>>,
    read_so_far: u64,
}

impl Package {
    pub fn open(data: &[u8]) -> Result<Self, PreviewError> {
        let zip = zip::ZipArchive::new(Cursor::new(data.to_vec())).map_err(|_| unreadable())?;
        if zip.len() > MAX_ENTRIES {
            return Err(PreviewError::TooLarge);
        }
        Ok(Self { zip, read_so_far: 0 })
    }

    /// One part's bytes, or `None` if it is missing. Breaking a limit is an error.
    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, PreviewError> {
        let name = name.trim_start_matches('/');
        let budget = self.budget();
        let entry = match self.zip.by_name(name) {
            Ok(e) => e,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(_) => return Err(unreadable()),
        };
        let out = read_capped(entry, budget)?;
        self.read_so_far += out.len() as u64;
        Ok(Some(out))
    }

    fn budget(&self) -> u64 {
        MAX_ENTRY_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(self.read_so_far))
    }

    pub fn read_text(&mut self, name: &str) -> Result<Option<String>, PreviewError> {
        Ok(self.read(name)?.map(|b| String::from_utf8_lossy(&b).into_owned()))
    }

    /// Decompress every entry within the limits (used before handing the
    /// package to a reader that has no limits of its own).
    pub fn check_all(&mut self) -> Result<(), PreviewError> {
        for i in 0..self.zip.len() {
            let budget = self.budget();
            let entry = self.zip.by_index(i).map_err(|_| unreadable())?;
            let out = read_capped(entry, budget)?;
            self.read_so_far += out.len() as u64;
        }
        Ok(())
    }
}

/// Read at most `budget` bytes of an entry; more is an error.
fn read_capped(entry: impl Read, budget: u64) -> Result<Vec<u8>, PreviewError> {
    let mut out = Vec::new();
    entry.take(budget + 1).read_to_end(&mut out).map_err(|_| unreadable())?;
    if out.len() as u64 > budget {
        return Err(PreviewError::TooLarge);
    }
    Ok(out)
}

fn unreadable() -> PreviewError {
    PreviewError::Unreadable("Office ファイルとして読み取れません".into())
}

/// A relationship target: an internal part path, or an external URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Part(String),
    External(String),
}

/// `rId → target` from a `_rels/*.rels` part. Internal targets are resolved
/// against `base_dir` (the folder of the part the relationships belong to).
pub(crate) fn relationships(xml: &str, base_dir: &str) -> HashMap<String, Target> {
    let mut map = HashMap::new();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == b"Relationship" => {
                let (mut id, mut target, mut external) = (None, None, false);
                for a in e.attributes().flatten() {
                    let value = a.unescape_value().map(|v| v.into_owned()).unwrap_or_default();
                    match a.key.local_name().as_ref() {
                        b"Id" => id = Some(value),
                        b"Target" => target = Some(value),
                        b"TargetMode" => external = value.eq_ignore_ascii_case("External"),
                        _ => {}
                    }
                }
                if let (Some(id), Some(target)) = (id, target) {
                    let t = if external { Target::External(target) } else { Target::Part(resolve(base_dir, &target)) };
                    map.insert(id, t);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

/// Join a relative part path onto a folder, folding `..` (never above the root).
pub(crate) fn resolve(base_dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') { Vec::new() } else { base_dir.split('/').filter(|p| !p.is_empty()).collect() };
    for piece in target.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// The `_rels` path for a part: `word/document.xml` → `word/_rels/document.xml.rels`.
pub(crate) fn rels_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Folder of a part path.
pub(crate) fn dir_of(part: &str) -> &str {
    part.rsplit_once('/').map_or("", |(d, _)| d)
}

/// An attribute's value by local name (namespace prefixes ignored).
pub(crate) fn attr(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// Build a zip package from (name, contents) pairs.
    pub(crate) fn package(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut out);
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, data) in files {
                zip.start_file(*name, options).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        out.into_inner()
    }

    #[test]
    fn resolves_paths_and_relationships() {
        assert_eq!(resolve("word", "media/a.png"), "word/media/a.png");
        assert_eq!(resolve("ppt/slides", "../media/b.png"), "ppt/media/b.png");
        assert_eq!(resolve("ppt/slides", "../../../../etc/passwd"), "etc/passwd");
        assert_eq!(resolve("word", "/customXml/x.xml"), "customXml/x.xml");
        assert_eq!(rels_path("word/document.xml"), "word/_rels/document.xml.rels");
        let rels = relationships(
            r#"<Relationships><Relationship Id="rId1" Target="media/a.png"/><Relationship Id="rId2" Target="https://t.example/x.png" TargetMode="External"/></Relationships>"#,
            "word",
        );
        assert_eq!(rels["rId1"], Target::Part("word/media/a.png".into()));
        assert_eq!(rels["rId2"], Target::External("https://t.example/x.png".into()));
    }

    #[test]
    fn zip_bombs_hit_the_read_limit() {
        // 64 MiB of zeros compresses to a few tens of kilobytes.
        let zeros = vec![0u8; 64 * 1024 * 1024];
        let data = package(&[("word/document.xml", &zeros)]);
        assert!(data.len() < 1024 * 1024);
        let mut p = Package::open(&data).unwrap();
        assert_eq!(p.read("word/document.xml"), Err(PreviewError::TooLarge));
        assert_eq!(p.read("missing.xml"), Ok(None));
    }
}
