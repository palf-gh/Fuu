//! Spreadsheets (xlsx, xlsm, xlsb, xls, ods) through calamine: each sheet as
//! a table of values with column letters and row numbers, merged cells kept.

use std::collections::HashMap;
use std::io::Cursor;

use calamine::{Data, DataType, Dimensions, Reader, Sheets};

use crate::html::{column_name, escape, note};
use crate::ooxml::Package;
use crate::{Output, PreviewError};

const MAX_SHEETS: usize = 12;
const MAX_ROWS: usize = 500;
const MAX_COLS: usize = 40;

pub(crate) fn preview(data: &[u8]) -> Result<Output, PreviewError> {
    // Zip-based workbooks are checked with our limits first: calamine itself
    // reads entries without any.
    if data.starts_with(b"PK") {
        Package::open(data)?.check_all()?;
    }
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
        let range = match book.worksheet_range(name) {
            Ok(r) => r,
            Err(e) => {
                out.html.push_str(&note(&format!("このシートは読み取れません（{}）", short(&e.to_string()))));
                continue;
            }
        };
        let merges = match &mut book {
            Sheets::Xlsx(x) => x.merge_cells_by_sheet_name(name).unwrap_or_default(),
            Sheets::Xls(x) => x.worksheet_merge_cells(name).unwrap_or_default(),
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
    fn number_formatting() {
        assert_eq!(number(120000.0), "120000");
        assert_eq!(number(0.1 + 0.2), "0.3");
        assert_eq!(number(-2.50), "-2.5");
    }

    #[test]
    fn broken_workbooks_are_errors() {
        assert!(preview(b"PK\x03\x04 not really").is_err());
        assert!(preview(b"plain text").is_err());
    }
}
