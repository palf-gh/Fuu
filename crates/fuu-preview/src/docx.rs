//! Word documents (docx): the body of `word/document.xml` as HTML. Kept:
//! headings, bold / italic / underline / strike, colour, size, highlight,
//! super/subscript, alignment and indents, numbered and bulleted lists (with
//! their own markers such as 「第1条」), tables with merged columns and
//! shading, embedded pictures, text boxes and web / mail links. Dropped:
//! headers, footers, footnotes, comments, deleted text, field codes and every
//! external reference (linked pictures, remote templates).

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::html::{colour, escape, note};
use crate::ooxml::{attr, dir_of, rels_path, relationships, Package, Target};
use crate::{Output, PreviewError};
use fuu_i18n::tr;

/// Longest side of an embedded picture.
const IMAGE_SIDE: u32 = 1600;
/// Pictures converted per document.
const MAX_IMAGES: usize = 60;
/// Paragraphs rendered.
const MAX_PARAGRAPHS: usize = 6000;

#[derive(Debug, Clone, Default)]
struct Level {
    format: String,
    text: String,
    start: u32,
}

/// numId → levels.
type Numbering = HashMap<String, Vec<Level>>;

fn numbering(xml: &str) -> Numbering {
    let mut abstracts: HashMap<String, Vec<Level>> = HashMap::new();
    let mut nums: HashMap<String, String> = HashMap::new();
    let mut reader = Reader::from_str(xml);
    let (mut current_abstract, mut current_num, mut level): (Option<String>, Option<String>, Option<usize>) = (None, None, None);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"abstractNum" => current_abstract = attr(&e, b"abstractNumId"),
                b"lvl" => level = attr(&e, b"ilvl").and_then(|v| v.parse().ok()).filter(|l: &usize| *l < 9),
                b"num" => current_num = attr(&e, b"numId"),
                b"abstractNumId" => {
                    if let (Some(num), Some(id)) = (&current_num, attr(&e, b"val")) {
                        nums.insert(num.clone(), id);
                    }
                }
                name @ (b"numFmt" | b"lvlText" | b"start") => {
                    if let (Some(a), Some(l)) = (&current_abstract, level) {
                        let levels = abstracts.entry(a.clone()).or_default();
                        if levels.len() <= l {
                            levels.resize(l + 1, Level::default());
                        }
                        let value = attr(&e, b"val").unwrap_or_default();
                        match name {
                            b"numFmt" => levels[l].format = value,
                            b"lvlText" => levels[l].text = value,
                            _ => levels[l].start = value.parse().unwrap_or(1),
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"abstractNum" => current_abstract = None,
                b"num" => current_num = None,
                b"lvl" => level = None,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    nums.into_iter().filter_map(|(num, a)| abstracts.get(&a).map(|l| (num, l.clone()))).collect()
}

/// paragraph styleId → heading level (1–6), from names like "heading 1" /
/// "Title" or an outline level.
fn headings(xml: &str) -> HashMap<String, u8> {
    let mut map = HashMap::new();
    let mut reader = Reader::from_str(xml);
    let mut current: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"style" => current = (attr(&e, b"type").as_deref() == Some("paragraph")).then(|| attr(&e, b"styleId")).flatten(),
                b"name" => {
                    if let (Some(id), Some(name)) = (&current, attr(&e, b"val")) {
                        let name = name.to_ascii_lowercase();
                        let level = if name == "title" {
                            Some(1)
                        } else {
                            name.strip_prefix("heading ").and_then(|n| n.trim().parse::<u8>().ok())
                        };
                        if let Some(l) = level.filter(|l| (1..=6).contains(l)) {
                            map.insert(id.clone(), l);
                        }
                    }
                }
                b"outlineLvl" => {
                    if let (Some(id), Some(l)) = (&current, attr(&e, b"val").and_then(|v| v.parse::<u8>().ok())) {
                        if l < 6 {
                            map.entry(id.clone()).or_insert(l + 1);
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.local_name().as_ref() == b"style" => current = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

fn roman(mut n: u32) -> String {
    const TABLE: &[(u32, &str)] = &[(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
    let mut s = String::new();
    for (v, r) in TABLE {
        while n >= *v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

fn letters(n: u32) -> String {
    let mut n = n.max(1) - 1;
    let mut s = Vec::new();
    loop {
        s.push(b'a' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

fn format_number(n: u32, format: &str) -> String {
    match format {
        "lowerLetter" => letters(n),
        "upperLetter" => letters(n).to_uppercase(),
        "lowerRoman" => roman(n),
        "upperRoman" => roman(n).to_uppercase(),
        "decimalFullWidth" | "decimalFullWidth2" => n.to_string().chars().map(|c| char::from_u32(c as u32 - '0' as u32 + 0xFF10).unwrap_or(c)).collect(),
        "decimalEnclosedCircle" | "decimalEnclosedCircleChinese" if (1..=20).contains(&n) => char::from_u32(0x2460 + n - 1).map(String::from).unwrap_or_default(),
        "aiueoFullWidth" | "aiueo" => {
            const KANA: &str = "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲン";
            KANA.chars().nth(((n.max(1) - 1) % 46) as usize).map(String::from).unwrap_or_default()
        }
        "iroha" | "irohaFullWidth" => {
            const IROHA: &str = "イロハニホヘトチリヌルヲワカヨタレソツネナラムウヰノオクヤマケフコエテアサキユメミシヱヒモセス";
            IROHA.chars().nth(((n.max(1) - 1) % 47) as usize).map(String::from).unwrap_or_default()
        }
        "none" => String::new(),
        _ => n.to_string(),
    }
}

#[derive(Default)]
struct Run {
    text: String,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    colour: Option<String>,
    size_pt: Option<f32>,
    highlight: Option<&'static str>,
    vert: Option<&'static str>,
}

impl Run {
    fn html(&self) -> String {
        if self.text.is_empty() {
            return String::new();
        }
        let mut style = String::new();
        if let Some(c) = &self.colour {
            style.push_str(&format!("color: {c}; "));
        }
        if let Some(s) = self.size_pt {
            style.push_str(&format!("font-size: {s}pt; "));
        }
        if let Some(h) = self.highlight {
            style.push_str(&format!("background-color: {h}; "));
        }
        let mut html = if style.is_empty() { self.text.clone() } else { format!("<span style=\"{}\">{}</span>", style.trim_end(), self.text) };
        for (on, tag) in [(self.bold, "b"), (self.italic, "i"), (self.underline, "u"), (self.strike, "s")] {
            if on {
                html = format!("<{tag}>{html}</{tag}>");
            }
        }
        if let Some(v) = self.vert {
            html = format!("<{v}>{html}</{v}>");
        }
        html
    }
}

fn highlight(name: &str) -> Option<&'static str> {
    Some(match name {
        "yellow" => "#fff59d",
        "green" => "#c5e1a5",
        "cyan" => "#b2ebf2",
        "magenta" => "#f8bbd0",
        "blue" => "#90caf9",
        "red" => "#ef9a9a",
        "lightGray" => "#e0e0e0",
        "darkGray" => "#9e9e9e",
        _ => return None,
    })
}

#[derive(Default)]
struct Para {
    html: String,
    style: Option<String>,
    align: Option<&'static str>,
    indent_px: Option<u32>,
    num: Option<(String, usize)>,
    page_break: bool,
}

/// A table cell whose `<td>` is written once its properties are known.
#[derive(Default)]
struct Cell {
    open: bool,
    colspan: u32,
    shade: Option<String>,
    continued: bool,
}

/// Is an on/off property (`<w:b/>`, `<w:b w:val="0"/>`) on?
fn on(e: &BytesStart<'_>) -> bool {
    !matches!(attr(e, b"val").as_deref(), Some("0" | "false" | "off" | "none"))
}

struct Converter<'p> {
    package: &'p mut Package,
    rels: HashMap<String, Target>,
    numbering: Numbering,
    headings: HashMap<String, u8>,
    counters: HashMap<String, Vec<u32>>,
    out: Output,
    paras: Vec<Para>,
    /// Paragraphs found inside another (text boxes), written after it.
    deferred: Vec<String>,
    run: Option<Run>,
    cells: Vec<Cell>,
    in_text: bool,
    skip_depth: usize,
    images: usize,
    dropped_external: usize,
    paragraphs: usize,
    truncated: bool,
    link_open: Vec<bool>,
    /// Width (EMU) of the picture whose blip comes next.
    extent: Option<u64>,
}

impl Converter<'_> {
    fn push_html(&mut self, html: &str) {
        if let Some(p) = self.paras.last_mut() {
            p.html.push_str(html);
        } else {
            self.out.html.push_str(html);
        }
    }

    fn flush_run(&mut self) {
        if let Some(run) = self.run.take() {
            let html = run.html();
            self.push_html(&html);
        }
    }

    fn open_cell(&mut self) {
        let Some(cell) = self.cells.last_mut() else { return };
        if cell.open {
            return;
        }
        cell.open = true;
        let mut attrs = String::new();
        if cell.colspan > 1 {
            attrs.push_str(&format!(" colspan=\"{}\"", cell.colspan.min(64)));
        }
        let mut style = String::new();
        if let Some(s) = &cell.shade {
            style.push_str(&format!("background-color: {s}; "));
        }
        if cell.continued {
            style.push_str("border-top: none; ");
        }
        if !style.is_empty() {
            attrs.push_str(&format!(" style=\"{}\"", style.trim_end()));
        }
        let html = format!("<td{attrs}>");
        self.out.html.push_str(&html);
    }

    fn image(&mut self, rel: &str, width_emu: Option<u64>) {
        match self.rels.get(rel).cloned() {
            Some(Target::External(_)) => self.dropped_external += 1,
            Some(Target::Part(path)) if self.images < MAX_IMAGES => {
                self.images += 1;
                let data = self.package.read(&path).ok().flatten();
                match data.and_then(|d| crate::image::embedded(&d, IMAGE_SIDE)) {
                    Some((mime, bytes)) => {
                        let src = self.out.add_image(mime, bytes);
                        let width = width_emu.map(|w| (w / 9525).clamp(8, 2000));
                        let size = width.map_or(String::new(), |w| format!(" width=\"{w}\""));
                        self.push_html(&format!("<img src=\"{src}\"{size} style=\"max-width: 100%; height: auto;\" alt=\"\">"));
                    }
                    None => self.push_html(&format!("<span style=\"color: #a3a19b;\">{}</span>", tr!("[Picture]"))),
                }
            }
            _ => {}
        }
    }

    fn end_paragraph(&mut self) {
        self.flush_run();
        let Some(p) = self.paras.pop() else { return };
        self.paragraphs += 1;
        let mut style = String::new();
        if let Some(a) = p.align {
            style.push_str(&format!("text-align: {a}; "));
        }
        let mut content = if p.html.trim().is_empty() { "<br>".to_owned() } else { p.html };
        let mut indent = p.indent_px.unwrap_or(0);
        if let Some((num_id, level)) = &p.num {
            if let Some(levels) = self.numbering.get(num_id) {
                let counters = self.counters.entry(num_id.clone()).or_insert_with(|| vec![0; 9]);
                counters[*level] += 1;
                for deeper in counters.iter_mut().skip(level + 1) {
                    *deeper = 0;
                }
                let lvl = levels.get(*level).cloned().unwrap_or_default();
                let marker = if lvl.format == "bullet" {
                    "•".to_owned()
                } else {
                    let mut text = if lvl.text.is_empty() { format!("%{}.", level + 1) } else { lvl.text.clone() };
                    for (i, l) in levels.iter().enumerate().take(level + 1).rev() {
                        let value = counters[i] + l.start.max(1) - 1;
                        text = text.replace(&format!("%{}", i + 1), &format_number(value, &l.format));
                    }
                    text
                };
                if !marker.is_empty() {
                    content = format!("{}\u{2003}{content}", escape(&marker));
                }
                indent = indent.max(24 * (*level as u32 + 1));
            }
        }
        if indent > 0 {
            style.push_str(&format!("margin-left: {}px; ", indent.min(400)));
        }
        let style_attr = if style.is_empty() { String::new() } else { format!(" style=\"{}\"", style.trim_end()) };
        let html = match p.style.as_ref().and_then(|s| self.headings.get(s)) {
            Some(level) if p.num.is_none() => format!("<h{level}{style_attr}>{content}</h{level}>"),
            _ => format!("<p{style_attr}>{content}</p>"),
        };
        let html = if p.page_break { format!("{html}<hr>") } else { html };
        if self.paras.is_empty() {
            self.out.html.push_str(&html);
            for d in std::mem::take(&mut self.deferred) {
                self.out.html.push_str(&d);
            }
        } else {
            self.deferred.push(html);
        }
    }

    fn start(&mut self, e: &BytesStart<'_>, empty: bool) {
        let name = e.local_name();
        let name = name.as_ref();
        if self.skip_depth > 0 {
            if !empty {
                self.skip_depth += 1;
            }
            return;
        }
        match name {
            // Duplicated fallback content, deleted text, field instructions.
            b"Fallback" | b"del" | b"instrText" | b"delText" | b"footnoteReference" | b"commentReference" => {
                if !empty {
                    self.skip_depth = 1;
                }
            }
            b"p" if !empty => {
                if self.paragraphs >= MAX_PARAGRAPHS {
                    self.truncated = true;
                    self.skip_depth = 1;
                    return;
                }
                if self.paras.is_empty() {
                    self.open_cell();
                }
                self.paras.push(Para::default());
            }
            b"pStyle" => {
                if let Some(p) = self.paras.last_mut() {
                    p.style = attr(e, b"val");
                }
            }
            b"jc" => {
                if let Some(p) = self.paras.last_mut() {
                    p.align = match attr(e, b"val").as_deref() {
                        Some("center") => Some("center"),
                        Some("right" | "end") => Some("right"),
                        Some("both" | "distribute") => Some("justify"),
                        _ => None,
                    };
                }
            }
            b"ind" if self.run.is_none() => {
                if let Some(p) = self.paras.last_mut() {
                    let twips: Option<u32> = attr(e, b"left").or_else(|| attr(e, b"start")).and_then(|v| v.parse().ok());
                    p.indent_px = twips.map(|t| t / 15);
                }
            }
            b"numId" => {
                if let (Some(p), Some(id)) = (self.paras.last_mut(), attr(e, b"val")) {
                    let level = p.num.as_ref().map_or(0, |n| n.1);
                    p.num = (id != "0").then_some((id, level));
                }
            }
            b"ilvl" => {
                if let Some(p) = self.paras.last_mut() {
                    let level = attr(e, b"val").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0).min(8);
                    p.num = Some((p.num.take().map(|n| n.0).unwrap_or_default(), level));
                }
            }
            b"r" if !empty => {
                self.flush_run();
                self.run = Some(Run::default());
            }
            b"b" | b"bCs" => {
                if let Some(r) = &mut self.run {
                    r.bold = on(e);
                }
            }
            b"i" | b"iCs" => {
                if let Some(r) = &mut self.run {
                    r.italic = on(e);
                }
            }
            b"u" => {
                if let Some(r) = &mut self.run {
                    r.underline = on(e);
                }
            }
            b"strike" | b"dstrike" => {
                if let Some(r) = &mut self.run {
                    r.strike = on(e);
                }
            }
            b"color" => {
                if let Some(r) = &mut self.run {
                    r.colour = attr(e, b"val").and_then(|v| colour(&v));
                }
            }
            b"sz" => {
                if let Some(r) = &mut self.run {
                    r.size_pt = attr(e, b"val").and_then(|v| v.parse::<f32>().ok()).map(|h| (h / 2.0).clamp(4.0, 96.0));
                }
            }
            b"highlight" => {
                if let Some(r) = &mut self.run {
                    r.highlight = attr(e, b"val").and_then(|v| highlight(&v));
                }
            }
            b"vertAlign" => {
                if let Some(r) = &mut self.run {
                    r.vert = match attr(e, b"val").as_deref() {
                        Some("superscript") => Some("sup"),
                        Some("subscript") => Some("sub"),
                        _ => None,
                    };
                }
            }
            b"t" if !empty => self.in_text = true,
            b"tab" if self.run.is_some() => {
                if let Some(r) = &mut self.run {
                    r.text.push('\u{2003}');
                }
            }
            b"br" | b"cr" => {
                if attr(e, b"type").as_deref() == Some("page") {
                    if let Some(p) = self.paras.first_mut() {
                        p.page_break = true;
                    }
                } else if let Some(r) = &mut self.run {
                    r.text.push_str("<br>");
                }
            }
            b"hyperlink" if !empty => {
                self.flush_run();
                let href = attr(e, b"id").and_then(|id| match self.rels.get(&id) {
                    Some(Target::External(url)) => Some(url.clone()),
                    _ => None,
                });
                let href = href.filter(|u| {
                    let l = u.to_ascii_lowercase();
                    l.starts_with("https://") || l.starts_with("http://") || l.starts_with("mailto:")
                });
                match href {
                    Some(url) => {
                        self.push_html(&format!("<a href=\"{}\">", escape(&url)));
                        self.link_open.push(true);
                    }
                    None => self.link_open.push(false),
                }
            }
            b"extent" => {
                // Picture size; remembered on the run until its blip arrives.
                if let Some(cx) = attr(e, b"cx") {
                    self.pending_extent(cx.parse().ok());
                }
            }
            b"blip" => {
                if let Some(id) = attr(e, b"embed") {
                    let width = self.extent.take();
                    self.flush_run();
                    self.image(&id, width);
                } else if attr(e, b"link").is_some() {
                    self.dropped_external += 1;
                }
            }
            b"imagedata" => {
                if let Some(id) = attr(e, b"id") {
                    self.flush_run();
                    self.image(&id, None);
                }
            }
            b"tbl" if !empty => {
                self.flush_run();
                self.out.html.push_str("<table>");
                self.cells.push(Cell::default());
            }
            b"tr" if !empty => self.out.html.push_str("<tr>"),
            b"tc" if !empty => {
                if let Some(c) = self.cells.last_mut() {
                    *c = Cell { colspan: 1, ..Cell::default() };
                }
            }
            b"gridSpan" => {
                if let Some(c) = self.cells.last_mut() {
                    c.colspan = attr(e, b"val").and_then(|v| v.parse().ok()).unwrap_or(1);
                }
            }
            b"vMerge" => {
                if let Some(c) = self.cells.last_mut() {
                    c.continued = attr(e, b"val").is_none_or(|v| v == "continue");
                }
            }
            b"shd" if self.run.is_none() && self.paras.is_empty() => {
                if let Some(c) = self.cells.last_mut() {
                    c.shade = attr(e, b"fill").and_then(|v| colour(&v));
                }
            }
            _ => {}
        }
    }

    fn pending_extent(&mut self, cx: Option<u64>) {
        self.extent = cx;
    }

    fn end(&mut self, name: &[u8]) {
        if self.skip_depth > 0 {
            self.skip_depth -= 1;
            return;
        }
        match name {
            b"t" => self.in_text = false,
            b"r" => self.flush_run(),
            b"p" => self.end_paragraph(),
            b"hyperlink" => {
                self.flush_run();
                if self.link_open.pop() == Some(true) {
                    self.push_html("</a>");
                }
            }
            b"tc" => {
                self.open_cell();
                self.out.html.push_str("</td>");
            }
            b"tr" => self.out.html.push_str("</tr>"),
            b"tbl" => {
                self.cells.pop();
                self.out.html.push_str("</table>");
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if self.skip_depth > 0 || !self.in_text {
            return;
        }
        if let Some(r) = &mut self.run {
            r.text.push_str(&escape(text));
        }
    }
}

// The picture extent travels between events; kept outside the struct literal
// above for readability.
impl Converter<'_> {
    #[allow(clippy::needless_lifetimes)]
    fn new<'p>(package: &'p mut Package, rels: HashMap<String, Target>, numbering: Numbering, headings: HashMap<String, u8>) -> Converter<'p> {
        Converter {
            package,
            rels,
            numbering,
            headings,
            counters: HashMap::new(),
            out: Output::default(),
            paras: Vec::new(),
            deferred: Vec::new(),
            run: None,
            cells: Vec::new(),
            in_text: false,
            skip_depth: 0,
            images: 0,
            dropped_external: 0,
            paragraphs: 0,
            truncated: false,
            link_open: Vec::new(),
            extent: None,
        }
    }
}

pub(crate) fn preview(data: &[u8]) -> Result<Output, PreviewError> {
    let mut package = Package::open(data)?;
    let document = "word/document.xml";
    let xml = package.read_text(document)?.ok_or_else(|| PreviewError::Unreadable(tr!("Not readable as a Word document").into()))?;
    let rels = package.read_text(&rels_path(document))?.map(|r| relationships(&r, dir_of(document))).unwrap_or_default();
    let numbering = package.read_text("word/numbering.xml")?.map(|x| numbering(&x)).unwrap_or_default();
    let headings = package.read_text("word/styles.xml")?.map(|x| headings(&x)).unwrap_or_default();

    let mut c = Converter::new(&mut package, rels, numbering, headings);
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(false);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => c.start(&e, false),
            Ok(Event::Empty(e)) => c.start(&e, true),
            Ok(Event::End(e)) => c.end(e.local_name().as_ref()),
            Ok(Event::Text(t)) => {
                if let Ok(text) = t.decode() {
                    let text = quick_xml::escape::unescape(&text).map(|t| t.into_owned()).unwrap_or_else(|_| text.into_owned());
                    c.text(&text);
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if let Ok(Some(ch)) = r.resolve_char_ref() {
                    c.text(ch.encode_utf8(&mut [0; 4]));
                } else if let Ok(name) = r.decode() {
                    let text = match name.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    };
                    c.text(text);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return Err(PreviewError::Unreadable(tr!("The Word document is damaged").into())),
            _ => {}
        }
    }
    while !c.paras.is_empty() {
        c.end_paragraph();
    }
    let mut notes = String::new();
    if c.truncated {
        notes.push_str(&note(tr!("A long document: only the beginning is shown")));
    }
    if c.dropped_external > 0 {
        notes.push_str(&note(&tr!("Pictures loaded from elsewhere, not shown: {0}", c.dropped_external)));
    }
    let mut out = c.out;
    out.html = format!("{notes}<div class=\"fuu-doc\">{}</div>", out.html);
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ooxml::tests::package;

    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;

    pub(crate) fn sample_docx() -> Vec<u8> {
        let png = {
            let mut out = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 4, image::Rgb([200, 30, 30])))
                .write_to(&mut out, image::ImageFormat::Png)
                .unwrap();
            out.into_inner()
        };
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:document {W}><w:body>
<w:p><w:pPr><w:pStyle w:val="1"/></w:pPr><w:r><w:t>業務委託契約書</w:t></w:r></w:p>
<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:b/><w:color w:val="C00000"/><w:sz w:val="28"/></w:rPr><w:t xml:space="preserve">重要 </w:t></w:r><w:r><w:t>&lt;script&gt;は文字&#x3042;</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr></w:pPr><w:r><w:t>目的</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr></w:pPr><w:r><w:t>期間</w:t></w:r></w:p>
<w:p><w:hyperlink r:id="rLink"><w:r><w:t>サイト</w:t></w:r></w:hyperlink><w:hyperlink r:id="rJs"><w:r><w:t>危険</w:t></w:r></w:hyperlink></w:p>
<w:p><w:r><w:drawing><wp:inline><wp:extent cx="952500" cy="476250"/><a:graphic><a:graphicData><a:blip r:embed="rImg"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r><w:r><w:drawing><a:blip r:link="rRemote"/></w:drawing></w:r></w:p>
<w:p><w:r><mc:AlternateContent><mc:Choice><w:t>表</w:t></mc:Choice><mc:Fallback><w:t>二重</w:t></mc:Fallback></mc:AlternateContent></w:r><w:del><w:r><w:delText>削除済み</w:delText></w:r></w:del></w:p>
<w:tbl><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/><w:shd w:fill="D9E2F3"/></w:tcPr><w:p><w:r><w:t>見出しセル</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:body></w:document>"#
        );
        package(&[
            ("word/document.xml", body.as_bytes()),
            ("word/_rels/document.xml.rels", br#"<Relationships><Relationship Id="rLink" Target="https://example.jp/" TargetMode="External"/><Relationship Id="rJs" Target="javascript:alert(1)" TargetMode="External"/><Relationship Id="rImg" Target="media/image1.png"/><Relationship Id="rRemote" Target="https://t.example/pixel.png" TargetMode="External"/></Relationships>"#),
            ("word/media/image1.png", &png),
            ("word/styles.xml", "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:style w:type=\"paragraph\" w:styleId=\"1\"><w:name w:val=\"heading 1\"/></w:style></w:styles>".as_bytes()),
            ("word/numbering.xml", "<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimalFullWidth\"/><w:lvlText w:val=\"第%1条\"/></w:lvl></w:abstractNum><w:num w:numId=\"3\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>".as_bytes()),
        ])
    }

    #[test]
    fn converts_structure_and_formatting() {
        if let Some(path) = std::env::var_os("FUU_PREVIEW_DUMP") {
            std::fs::write(path, sample_docx()).unwrap();
        }
        let out = preview(&sample_docx()).unwrap();
        let h = &out.html;
        assert!(h.contains("<h1>業務委託契約書</h1>"), "{h}");
        assert!(h.contains("text-align: center"), "{h}");
        assert!(h.contains("<b><span style=\"color: #c00000; font-size: 14pt;\">重要 </span></b>"), "{h}");
        assert!(h.contains("&lt;script&gt;は文字あ"), "text stays text: {h}");
        assert!(h.contains("第１条\u{2003}目的") && h.contains("第２条\u{2003}期間"), "{h}");
        assert!(h.contains("<a href=\"https://example.jp/\">サイト</a>"), "{h}");
        assert!(!h.contains("javascript"), "{h}");
        assert!(h.contains("危険"), "the link text is kept: {h}");
        assert_eq!(out.images.len(), 1, "embedded picture only");
        assert!(h.contains("width=\"100\""), "{h}");
        assert!(h.contains("not shown: 1"), "{h}");
        assert!(h.contains("表") && !h.contains("二重") && !h.contains("削除済み"), "{h}");
        assert!(h.contains("colspan=\"2\"") && h.contains("background-color: #d9e2f3"), "{h}");
    }

    #[test]
    fn numbering_formats() {
        assert_eq!(format_number(4, "upperRoman"), "IV");
        assert_eq!(format_number(28, "lowerLetter"), "ab");
        assert_eq!(format_number(3, "decimalEnclosedCircle"), "③");
        assert_eq!(format_number(2, "aiueoFullWidth"), "イ");
    }

    #[test]
    fn not_a_word_file() {
        assert!(preview(b"not a zip").is_err());
        let empty = package(&[("other.xml", b"<x/>")]);
        assert!(preview(&empty).is_err());
    }
}
