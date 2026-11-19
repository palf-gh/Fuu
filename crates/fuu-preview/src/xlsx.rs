//! Spreadsheets (xlsx, xlsm) through calamine: each sheet as a table of
//! values with column letters and row numbers, merged cells kept.
//!
//! calamine stores a sheet as a dense grid of its bounding box, so a tiny file
//! with one cell in A1 and one in XFD1048576 would ask for hundreds of
//! gigabytes and abort the process (not a catchable panic). Every sheet's
//! bounding box is therefore measured from the XML first, and sheets over
//! [`MAX_CELLS`] are skipped. Binary and ODF workbooks (xls, xlsb, ods) cannot
//! be measured cheaply, so they are not previewed.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;

use calamine::{Data, DataType, Dimensions, Reader, Sheets};
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;

use crate::html::{column_name, escape, note};
use crate::ooxml::{attr, dir_of, rels_path, relationships, Package, Target};
use crate::{Output, PreviewError};

const MAX_SHEETS: usize = 12;
const MAX_ROWS: usize = 500;
const MAX_COLS: usize = 40;
/// Largest bounding box (cells) handed to calamine for one sheet.
const MAX_CELLS: u64 = 2_000_000;
/// Largest shared-string count accepted (calamine reserves this many up front).
const MAX_STRINGS: u64 = 2_000_000;

/// Refuse shared-string tables whose declared counts would make calamine
/// reserve absurd amounts of memory. Every part is checked, whatever its name.
fn check_shared_strings(package: &mut Package) -> Result<(), PreviewError> {
    for name in package.names() {
        if !name.to_ascii_lowercase().ends_with(".xml") {
            continue;
        }
        let Some(xml) = package.read_text(&name)? else { continue };
        let mut reader = XmlReader::from_str(&xml);
        loop {
            match reader.read_event() {
                Ok(Event::Start(e) | Event::Empty(e)) => {
                    if e.local_name().as_ref() == b"sst" {
                        for key in [&b"uniqueCount"[..], b"count"] {
                            let n = attr(&e, key).map(|v| v.trim().parse::<u64>().unwrap_or(u64::MAX)).unwrap_or(0);
                            if n > MAX_STRINGS {
                                return Err(PreviewError::TooLarge);
                            }
                        }
                    }
                    break;
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
    }
    Ok(())
}

/// `A1`-style reference → (row, column), both 1-based.
fn cell_ref(r: &str) -> Option<(u64, u64)> {
    let letters = r.bytes().take_while(u8::is_ascii_alphabetic).count();
    if letters == 0 || letters > 3 {
        return None;
    }
    let col = r[..letters].bytes().fold(0u64, |acc, b| acc * 26 + u64::from(b.to_ascii_uppercase() - b'A' + 1));
    let row: u64 = r[letters..].parse().ok()?;
    Some((row, col))
}

/// Bounding box size (cells) of a worksheet part, from its cell references
/// (cells without one continue the previous position).
fn bounding_cells(xml: &str) -> u64 {
    let (mut min_r, mut max_r, mut min_c, mut max_c) = (u64::MAX, 0u64, u64::MAX, 0u64);
    let (mut row, mut col) = (0u64, 0u64);
    let mut reader = XmlReader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => match e.local_name().as_ref() {
                b"row" => {
                    row = attr(&e, b"r").and_then(|v| v.parse().ok()).unwrap_or(row + 1);
                    col = 0;
                }
                b"c" => {
                    match attr(&e, b"r").and_then(|v| cell_ref(&v)) {
                        Some((r, c)) => {
                            row = r;
                            col = c;
                        }
                        None => col += 1,
                    }
                    min_r = min_r.min(row);
                    max_r = max_r.max(row);
                    min_c = min_c.min(col);
                    max_c = max_c.max(col);
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    if max_r == 0 || max_c == 0 {
        return 0;
    }
    (max_r - min_r.min(max_r) + 1).saturating_mul(max_c - min_c.min(max_c) + 1)
}

/// Names of sheets whose bounding box is too large to load.
fn oversized_sheets(package: &mut Package) -> Result<HashSet<String>, PreviewError> {
    let book = "xl/workbook.xml";
    let Some(xml) = package.read_text(book)? else { return Ok(HashSet::new()) };
    let rels = package.read_text(&rels_path(book))?.map(|r| relationships(&r, dir_of(book))).unwrap_or_default();
    let mut sheets = Vec::new();
    let mut reader = XmlReader::from_str(&xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == b"sheet" => {
                let name = attr(&e, b"name").unwrap_or_default();
                let rel = e
                    .attributes()
                    .flatten()
                    .find(|a| a.key.prefix().is_some() && a.key.local_name().as_ref() == b"id")
                    .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()));
                if let Some(Target::Part(path)) = rel.and_then(|id| rels.get(&id).cloned()) {
                    sheets.push((name, path));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let mut oversized = HashSet::new();
    for (name, path) in sheets {
        if let Some(xml) = package.read_text(&path)? {
            if bounding_cells(&xml) > MAX_CELLS {
                oversized.insert(name);
            }
        }
    }
    Ok(oversized)
}

pub(crate) fn preview(data: &[u8]) -> Result<Output, PreviewError> {
    // Checked with our limits first: calamine itself reads zip entries
    // without any, and loads each sheet as a dense grid.
    if !data.starts_with(b"PK") {
        return Err(PreviewError::Unsupported);
    }
    let mut package = Package::open(data)?;
    package.check_all()?;
    check_shared_strings(&mut package)?;
    let oversized = oversized_sheets(&mut package)?;
    let mut book = calamine::open_workbook_auto_from_rs(Cursor::new(data.to_vec()))
        .map_err(|e| PreviewError::Unreadable(short(&e.to_string())))?;
    let names = book.sheet_names().to_vec();
    if names.is_empty() {
        return Err(PreviewError::Unreadable("シートがありません".into()));
    }
    let mut out = Output::default();
    if names.len() > MAX_SHEETS {
        out.html.push_str(&note(&format!("全 {} シートのうち、最初の {MAX_SHEETS} シートを表示しています", names.len())));
    }
    for name in names.iter().take(MAX_SHEETS) {
        out.html.push_str(&format!("<p class=\"fuu-tabs\">{}</p>", escape(name)));
        if oversized.contains(name) {
            out.html.push_str(&note("このシートは範囲が広すぎるため表示しません"));
            continue;
        }
        let range = match book.worksheet_range(name) {
            Ok(r) => r,
            Err(e) => {
                out.html.push_str(&note(&format!("このシートは読み取れません（{}）", short(&e.to_string()))));
                continue;
            }
        };
        let merges = match &mut book {
            Sheets::Xlsx(x) => x.merge_cells_by_sheet_name(name).unwrap_or_default(),
            _ => Vec::new(),
        };
        out.html.push_str(&sheet_table(&range, &merges));
    }
    Ok(out)
}

fn short(message: &str) -> String {
    message.chars().take(120).collect()
}

fn sheet_table(range: &calamine::Range<Data>, merges: &[Dimensions]) -> String {
    let Some((row0, col0)) = range.start() else {
        return note("（空のシート）");
    };
    let (height, width) = range.get_size();
    let (rows, cols) = (height.min(MAX_ROWS), width.min(MAX_COLS));
    // Merged regions in range-relative coordinates: top-left → span, others hidden.
    let mut spans: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
    let mut hidden = std::collections::HashSet::new();
    for m in merges {
        let (Some(r), Some(c)) = (m.start.0.checked_sub(row0), m.start.1.checked_sub(col0)) else { continue };
        let (r, c) = (r as usize, c as usize);
        let rs = (m.end.0.saturating_sub(m.start.0) as usize + 1).min(rows.saturating_sub(r).max(1));
        let cs = (m.end.1.saturating_sub(m.start.1) as usize + 1).min(cols.saturating_sub(c).max(1));
        spans.insert((r, c), (rs, cs));
        for dr in 0..rs {
            for dc in 0..cs {
                if dr + dc > 0 {
                    hidden.insert((r + dr, c + dc));
                }
            }
        }
    }
    let mut html = String::from("<table class=\"fuu-sheet\"><tr><th></th>");
    for c in 0..cols {
        html.push_str(&format!("<th>{}</th>", column_name(col0 as usize + c)));
    }
    html.push_str("</tr>");
    for r in 0..rows {
        html.push_str(&format!("<tr><th>{}</th>", row0 as usize + r + 1));
        for c in 0..cols {
            if hidden.contains(&(r, c)) {
                continue;
            }
            let span = spans.get(&(r, c)).map_or(String::new(), |(rs, cs)| {
                let mut s = String::new();
                if *rs > 1 {
                    s.push_str(&format!(" rowspan=\"{rs}\""));
                }
                if *cs > 1 {
                    s.push_str(&format!(" colspan=\"{cs}\""));
                }
                s
            });
            let cell = range.get((r, c)).unwrap_or(&Data::Empty);
            let (text, numeric) = cell_text(cell);
            let class = if numeric { " class=\"num\"" } else { "" };
            html.push_str(&format!("<td{class}{span}>{}</td>", escape(&text)));
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
    if height > rows || width > cols {
        html.push_str(&note(&format!("{height} 行 × {width} 列のうち、{rows} 行 × {cols} 列を表示しています")));
    }
    html
}

/// Display text of a cell, and whether it is a number (right-aligned).
fn cell_text(cell: &Data) -> (String, bool) {
    match cell {
        Data::Empty => (String::new(), false),
        Data::String(s) => (s.clone(), false),
        Data::Int(i) => (i.to_string(), true),
        Data::Float(f) => (number(*f), true),
        Data::Bool(b) => ((if *b { "TRUE" } else { "FALSE" }).to_owned(), false),
        Data::DateTime(d) => match cell.as_datetime() {
            Some(dt) if d.is_datetime() => {
                let text = if dt.time() == chrono::NaiveTime::MIN {
                    dt.format("%Y/%m/%d").to_string()
                } else if dt.date() == chrono::NaiveDate::from_ymd_opt(1899, 12, 31).unwrap_or_default()
                    || dt.date() == chrono::NaiveDate::from_ymd_opt(1899, 12, 30).unwrap_or_default()
                {
                    dt.format("%H:%M").to_string()
                } else {
                    dt.format("%Y/%m/%d %H:%M").to_string()
                };
                (text, true)
            }
            _ => (number(d.as_f64()), true),
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => (s.clone(), true),
        Data::Error(e) => (format!("#{e:?}"), false),
    }
}

/// Numbers as Excel shows them by default: no trailing zeros, at most 10
/// significant decimals.
fn number(f: f64) -> String {
    if !f.is_finite() {
        return f.to_string();
    }
    if f.fract() == 0.0 && f.abs() < 1e15 {
        return format!("{}", f as i64);
    }
    let s = format!("{:.10}", f);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ooxml::tests::package;

    fn workbook(cells: &str, merges: &str) -> Vec<u8> {
        let sheet = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{cells}</sheetData>{merges}</worksheet>"#
        );
        package(&[
            ("[Content_Types].xml", br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#),
            ("_rels/.rels", br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#),
            ("xl/workbook.xml", "<?xml version=\"1.0\"?><workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"見積\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>".as_bytes()),
            ("xl/_rels/workbook.xml.rels", br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ])
    }

    #[test]
    fn sheets_become_tables_with_merges() {
        let cells = r#"<row r="1"><c r="A1" t="inlineStr"><is><t>品名 &lt;b&gt;</t></is></c><c r="C1" t="inlineStr"><is><t>金額</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>導入支援</t></is></c><c r="C2"><v>120000</v></c></row><row r="3"><c r="C3"><v>0.125</v></c></row>"#;
        let out = preview(&workbook(cells, r#"<mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells>"#)).unwrap();
        assert!(out.html.contains("見積"), "{}", out.html);
        assert!(out.html.contains("品名 &lt;b&gt;"), "escaped: {}", out.html);
        assert!(out.html.contains("colspan=\"2\""), "{}", out.html);
        assert!(out.html.contains("<td class=\"num\">120000</td>"), "{}", out.html);
        assert!(out.html.contains("0.125"));
        assert!(out.html.contains("<th>C</th>"));
    }

    #[test]
    fn sparse_giant_sheets_are_skipped_not_allocated() {
        // One cell in A1 and one in XFD1048576: calamine would ask for ~550 GB.
        let cells = r#"<row r="1"><c r="A1"><v>1</v></c></row><row r="1048576"><c r="XFD1048576"><v>2</v></c></row>"#;
        let out = preview(&workbook(cells, "")).unwrap();
        assert!(out.html.contains("範囲が広すぎる"), "{}", out.html);
        assert_eq!(bounding_cells(&format!("<sheetData>{cells}</sheetData>")), 1_048_576 * 16_384);
        assert_eq!(cell_ref("XFD1048576"), Some((1_048_576, 16_384)));
        assert_eq!(cell_ref("B3"), Some((3, 2)));
        // Cells without references count on from the previous one.
        assert_eq!(bounding_cells(r#"<row r="2"><c/><c/><c/></row>"#), 3);
    }

    #[test]
    fn inflated_shared_string_counts_are_refused() {
        let mut files = vec![];
        let book = workbook(r#"<row r="1"><c r="A1" t="s"><v>0</v></c></row>"#, "");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(book)).unwrap();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).unwrap();
            let mut data = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut data).unwrap();
            files.push((f.name().unwrap().into_owned(), data));
        }
        files.push(("xl/strings-elsewhere.xml".into(), br#"<sst uniqueCount="9000000000000" count="1"><si><t>x</t></si></sst>"#.to_vec()));
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        assert!(matches!(preview(&package(&refs)), Err(PreviewError::TooLarge)));
    }

    #[test]
    fn number_formatting() {
        assert_eq!(number(120000.0), "120000");
        assert_eq!(number(0.1 + 0.2), "0.3");
        assert_eq!(number(-2.50), "-2.5");
    }

    #[test]
    fn broken_workbooks_are_errors() {
        assert!(preview(b"PK\x03\x04 not really").is_err());
        assert!(matches!(preview(b"\xd0\xcf\x11\xe0 legacy xls"), Err(PreviewError::Unsupported)));
    }
}
