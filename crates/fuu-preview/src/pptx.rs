//! Presentations (pptx): every slide as a card with its title, text (bullets
//! and levels, bold / italic / underline, colour, size), pictures and tables,
//! in reading order (top to bottom, then left to right). Animations, notes,
//! masters' decoration and external references are left out.

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::html::{colour, escape, note};
use crate::ooxml::{attr, dir_of, rels_path, relationships, Package, Target};
use crate::{Output, PreviewError};

const MAX_SLIDES: usize = 60;
const MAX_IMAGES: usize = 80;
const IMAGE_SIDE: u32 = 1600;

/// One shape's content and where it sits on the slide.
struct Block {
    y: i64,
    x: i64,
    title: bool,
    html: String,
}

#[derive(Default)]
struct RunStyle {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    colour: Option<String>,
    size_pt: Option<f32>,
}

fn run_html(text: &str, s: &RunStyle) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut style = String::new();
    if let Some(c) = &s.colour {
        style.push_str(&format!("color: {c}; "));
    }
    if let Some(size) = s.size_pt {
        style.push_str(&format!("font-size: {size}pt; "));
    }
    let mut html = if style.is_empty() { text.to_owned() } else { format!("<span style=\"{}\">{text}</span>", style.trim_end()) };
    for (on, tag) in [(s.bold, "b"), (s.italic, "i"), (s.underline, "u"), (s.strike, "s")] {
        if on {
            html = format!("<{tag}>{html}</{tag}>");
        }
    }
    html
}

fn on(e: &BytesStart<'_>, name: &[u8]) -> Option<bool> {
    attr(e, name).map(|v| v == "1" || v == "true")
}

struct Slide<'a> {
    rels: &'a HashMap<String, Target>,
    blocks: Vec<Block>,
    depth: usize,
    /// Depth of the shape being read (sp / pic / graphicFrame).
    shape: Option<usize>,
    block: Option<Block>,
    placeholder: Option<String>,
    para: Option<(String, usize, bool)>,
    para_bullet: Option<String>,
    run: Option<(String, RunStyle)>,
    in_text: bool,
    in_spec: bool,
    cell_open: bool,
    skip_cell: bool,
    pending_images: Vec<(String, Option<i64>)>,
    dropped_external: usize,
}

impl Slide<'_> {
    fn start(&mut self, e: &BytesStart<'_>, empty: bool) {
        if !empty {
            self.depth += 1;
        }
        let name = e.local_name();
        match name.as_ref() {
            b"sp" | b"pic" | b"graphicFrame" | b"cxnSp" if self.shape.is_none() && !empty => {
                self.shape = Some(self.depth);
                self.block = Some(Block { y: i64::MAX, x: i64::MAX, title: false, html: String::new() });
                self.placeholder = None;
            }
            b"ph" => {
                let kind = attr(e, b"type").unwrap_or_else(|| "body".into());
                if let Some(b) = &mut self.block {
                    b.title = matches!(kind.as_str(), "title" | "ctrTitle");
                }
                self.placeholder = Some(kind);
            }
            b"off" => {
                if let Some(b) = &mut self.block {
                    if b.y == i64::MAX {
                        b.x = attr(e, b"x").and_then(|v| v.parse().ok()).unwrap_or(0);
                        b.y = attr(e, b"y").and_then(|v| v.parse().ok()).unwrap_or(0);
                    }
                }
            }
            b"ext" if self.block.is_some() => {
                // Width of a picture (the first extent of the shape).
                if let Some(last) = self.pending_images.last_mut() {
                    if last.1.is_none() {
                        last.1 = attr(e, b"cx").and_then(|v| v.parse().ok());
                    }
                }
            }
            b"blip" => {
                if let Some(id) = attr(e, b"embed") {
                    self.pending_images.push((id, None));
                } else if attr(e, b"link").is_some() {
                    self.dropped_external += 1;
                }
            }
            b"p" if !empty && self.block.is_some() => {
                self.para = Some((String::new(), 0, false));
                self.para_bullet = None;
            }
            b"pPr" => {
                if let Some(p) = &mut self.para {
                    p.1 = attr(e, b"lvl").and_then(|v| v.parse().ok()).unwrap_or(0).min(8);
                }
            }
            b"buNone" => self.para_bullet = Some(String::new()),
            b"buChar" => self.para_bullet = Some(attr(e, b"char").unwrap_or_else(|| "•".into())),
            b"buAutoNum" => self.para_bullet = Some("・".into()),
            b"r" | b"fld" if !empty && self.para.is_some() => self.run = Some((String::new(), RunStyle::default())),
            b"rPr" => {
                if let Some((_, s)) = &mut self.run {
                    s.bold = on(e, b"b").unwrap_or(false);
                    s.italic = on(e, b"i").unwrap_or(false);
                    s.underline = attr(e, b"u").is_some_and(|u| u != "none");
                    s.strike = attr(e, b"strike").is_some_and(|v| v != "noStrike");
                    s.size_pt = attr(e, b"sz").and_then(|v| v.parse::<f32>().ok()).map(|v| (v / 100.0 * 0.75).clamp(6.0, 60.0));
                    self.in_spec = true;
                }
            }
            b"srgbClr" if self.in_spec => {
                if let Some((_, s)) = &mut self.run {
                    s.colour = attr(e, b"val").and_then(|v| colour(&v));
                }
            }
            b"t" if !empty => self.in_text = true,
            b"br" => {
                if let Some(p) = &mut self.para {
                    p.0.push_str("<br>");
                }
            }
            b"tbl" if !empty => self.push("<table>"),
            b"tr" if !empty => self.push("<tr>"),
            b"tc" if !empty => {
                let merged = attr(e, b"hMerge").is_some() || attr(e, b"vMerge").is_some();
                self.skip_cell = merged;
                if !merged {
                    let mut attrs = String::new();
                    if let Some(n) = attr(e, b"gridSpan").and_then(|v| v.parse::<u32>().ok()).filter(|n| *n > 1) {
                        attrs.push_str(&format!(" colspan=\"{}\"", n.min(64)));
                    }
                    if let Some(n) = attr(e, b"rowSpan").and_then(|v| v.parse::<u32>().ok()).filter(|n| *n > 1) {
                        attrs.push_str(&format!(" rowspan=\"{}\"", n.min(256)));
                    }
                    self.push(&format!("<td{attrs}>"));
                    self.cell_open = true;
                }
            }
            _ => {}
        }
    }

    fn push(&mut self, html: &str) {
        if let Some(b) = &mut self.block {
            b.html.push_str(html);
        }
    }

    fn end(&mut self, name: &[u8], package: &mut Package, out: &mut Output, images: &mut usize) {
        match name {
            b"t" => self.in_text = false,
            b"rPr" => self.in_spec = false,
            b"r" | b"fld" => {
                if let (Some((text, style)), Some(p)) = (self.run.take(), &mut self.para) {
                    p.0.push_str(&run_html(&text, &style));
                }
            }
            b"p" => {
                if let Some((content, level, _)) = self.para.take() {
                    if !self.skip_cell {
                        let html = self.paragraph(content, level);
                        self.push(&html);
                    }
                }
            }
            b"tc" => {
                if self.cell_open {
                    self.push("</td>");
                }
                self.cell_open = false;
                self.skip_cell = false;
            }
            b"tr" => self.push("</tr>"),
            b"tbl" => self.push("</table>"),
            _ => {}
        }
        if self.shape == Some(self.depth) {
            self.shape = None;
            if let Some(mut block) = self.block.take() {
                for (id, width) in std::mem::take(&mut self.pending_images) {
                    block.html.push_str(&self.picture(&id, width, package, out, images));
                }
                if !block.html.is_empty() {
                    self.blocks.push(block);
                }
            }
        }
        self.depth = self.depth.saturating_sub(1);
    }

    fn paragraph(&self, content: String, level: usize) -> String {
        if content.trim().is_empty() {
            return String::new();
        }
        let title = self.block.as_ref().is_some_and(|b| b.title);
        if title {
            return format!("<h2>{content}</h2>");
        }
        let body = matches!(self.placeholder.as_deref(), Some("body" | "obj"));
        let bullet = match &self.para_bullet {
            Some(b) => b.clone(),
            None if body => "•".into(),
            None => String::new(),
        };
        let indent = level * 22 + if bullet.is_empty() { 0 } else { 4 };
        let marker = if bullet.is_empty() { String::new() } else { format!("{}\u{2002}", escape(&bullet)) };
        let style = if indent > 0 { format!(" style=\"margin-left: {indent}px;\"") } else { String::new() };
        format!("<p{style}>{marker}{content}</p>")
    }

    fn picture(&mut self, id: &str, width_emu: Option<i64>, package: &mut Package, out: &mut Output, images: &mut usize) -> String {
        match self.rels.get(id) {
            Some(Target::External(_)) => {
                self.dropped_external += 1;
                String::new()
            }
            Some(Target::Part(path)) if *images < MAX_IMAGES => {
                *images += 1;
                let data = package.read(path).ok().flatten();
                match data.and_then(|d| crate::image::embedded(&d, IMAGE_SIDE)) {
                    Some((mime, bytes)) => {
                        let src = out.add_image(mime, bytes);
                        let width = width_emu.map(|w| (w / 9525).clamp(8, 1400));
                        let size = width.map_or(String::new(), |w| format!(" width=\"{w}\""));
                        format!("<p><img src=\"{src}\"{size} alt=\"\"></p>")
                    }
                    None => "<p style=\"color: #a3a19b;\">[図]</p>".into(),
                }
            }
            _ => String::new(),
        }
    }

    fn text(&mut self, text: &str) {
        if !self.in_text {
            return;
        }
        if let Some((t, _)) = &mut self.run {
            t.push_str(&escape(text));
        }
    }
}

/// Slide part paths in presentation order.
fn slide_paths(package: &mut Package) -> Result<Vec<String>, PreviewError> {
    let main = "ppt/presentation.xml";
    let xml = package.read_text(main)?.ok_or_else(|| PreviewError::Unreadable("PowerPoint ファイルとして読み取れません".into()))?;
    let rels = package.read_text(&rels_path(main))?.map(|r| relationships(&r, dir_of(main))).unwrap_or_default();
    let mut paths = Vec::new();
    let mut reader = Reader::from_str(&xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == b"sldId" => {
                // r:id (the relationship), not the bare numeric id.
                let rel = e
                    .attributes()
                    .flatten()
                    .find(|a| a.key.prefix().is_some() && a.key.local_name().as_ref() == b"id")
                    .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()));
                if let Some(Target::Part(p)) = rel.and_then(|id| rels.get(&id).cloned()) {
                    paths.push(p);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    Ok(paths)
}

pub(crate) fn preview(data: &[u8]) -> Result<Output, PreviewError> {
    let mut package = Package::open(data)?;
    let paths = slide_paths(&mut package)?;
    if paths.is_empty() {
        return Err(PreviewError::Unreadable("スライドがありません".into()));
    }
    let mut out = Output::default();
    let mut images = 0;
    let mut dropped = 0;
    let mut cards = String::new();
    for (n, path) in paths.iter().take(MAX_SLIDES).enumerate() {
        let Some(xml) = package.read_text(path)? else { continue };
        let rels = package.read_text(&rels_path(path))?.map(|r| relationships(&r, dir_of(path))).unwrap_or_default();
        let mut slide = Slide {
            rels: &rels,
            blocks: Vec::new(),
            depth: 0,
            shape: None,
            block: None,
            placeholder: None,
            para: None,
            para_bullet: None,
            run: None,
            in_text: false,
            in_spec: false,
            cell_open: false,
            skip_cell: false,
            pending_images: Vec::new(),
            dropped_external: 0,
        };
        let mut reader = Reader::from_str(&xml);
        reader.config_mut().trim_text(false);
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => slide.start(&e, false),
                Ok(Event::Empty(e)) => slide.start(&e, true),
                Ok(Event::End(e)) => slide.end(e.local_name().as_ref(), &mut package, &mut out, &mut images),
                Ok(Event::Text(t)) => {
                    if let Ok(text) = t.decode() {
                        slide.text(&text);
                    }
                }
                Ok(Event::GeneralRef(r)) => {
                    if let Ok(Some(ch)) = r.resolve_char_ref() {
                        slide.text(ch.encode_utf8(&mut [0; 4]));
                    } else if let Ok(name) = r.decode() {
                        let text = match name.as_ref() {
                            "amp" => "&",
                            "lt" => "<",
                            "gt" => ">",
                            "quot" => "\"",
                            "apos" => "'",
                            _ => "",
                        };
                        slide.text(text);
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }
        dropped += slide.dropped_external;
        let mut blocks = slide.blocks;
        // Titles first, then reading order; rows within ~0.5 cm count as one line.
        blocks.sort_by_key(|b| (!b.title, b.y / 180_000, b.x));
        let body: String = blocks.into_iter().map(|b| b.html).collect();
        cards.push_str(&format!(
            "<div class=\"fuu-slide\"><p class=\"fuu-slide-no\">スライド {}</p>{body}</div>",
            n + 1
        ));
    }
    if paths.len() > MAX_SLIDES {
        out.html.push_str(&note(&format!("全 {} 枚のうち、最初の {MAX_SLIDES} 枚を表示しています", paths.len())));
    } else {
        out.html.push_str(&note(&format!("{} 枚のスライド", paths.len())));
    }
    if dropped > 0 {
        out.html.push_str(&note(&format!("外部から読み込む画像 {dropped} 件は表示していません")));
    }
    out.html.push_str(&cards);
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ooxml::tests::package;

    const NS: &str = r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

    pub(crate) fn sample_pptx() -> Vec<u8> {
        let png = {
            let mut out = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(16, 9, image::Rgb([40, 80, 200])))
                .write_to(&mut out, image::ImageFormat::Png)
                .unwrap();
            out.into_inner()
        };
        let slide1 = format!(
            r#"<p:sld {NS}><p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="500000" y="1800000"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:rPr lang="ja-JP" b="1" sz="2400"><a:solidFill><a:srgbClr val="1F4E79"/></a:solidFill></a:rPr><a:t>背景</a:t></a:r></a:p><a:p><a:pPr lvl="1"/><a:r><a:t>現行システムの課題 &lt;x&gt;</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="500000" y="300000"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:t>新メール基盤の提案</a:t></a:r></a:p></p:txBody></p:sp>
<p:pic><p:blipFill><a:blip r:embed="rImg"/></p:blipFill><p:spPr><a:xfrm><a:off x="6000000" y="1800000"/><a:ext cx="2857500" cy="1607344"/></a:xfrm></p:spPr></p:pic>
<p:pic><p:blipFill><a:blip r:link="rRemote"/></p:blipFill></p:pic>
</p:spTree></p:cSld></p:sld>"#
        );
        let slide2 = format!(
            r#"<p:sld {NS}><p:cSld><p:spTree><p:graphicFrame><p:xfrm><a:off x="0" y="0"/></p:xfrm><a:graphic><a:graphicData><a:tbl><a:tr><a:tc gridSpan="2"><a:txBody><a:p><a:r><a:t>費用</a:t></a:r></a:p></a:txBody></a:tc><a:tc hMerge="1"><a:txBody><a:p><a:r><a:t>x</a:t></a:r></a:p></a:txBody></a:tc></a:tr><a:tr><a:tc><a:txBody><a:p><a:r><a:t>初期</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>¥1,200,000</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame></p:spTree></p:cSld></p:sld>"#
        );
        package(&[
            ("ppt/presentation.xml", format!(r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="256" r:id="rId2"/><p:sldId id="257" r:id="rId3"/></p:sldIdLst></p:presentation>"#).as_bytes()),
            ("ppt/_rels/presentation.xml.rels", br#"<Relationships><Relationship Id="rId2" Target="slides/slide1.xml"/><Relationship Id="rId3" Target="slides/slide2.xml"/></Relationships>"#),
            ("ppt/slides/slide1.xml", slide1.as_bytes()),
            ("ppt/slides/_rels/slide1.xml.rels", br#"<Relationships><Relationship Id="rImg" Target="../media/image1.png"/><Relationship Id="rRemote" Target="https://t.example/p.png" TargetMode="External"/></Relationships>"#),
            ("ppt/slides/slide2.xml", slide2.as_bytes()),
            ("ppt/media/image1.png", &png),
        ])
    }

    #[test]
    fn slides_become_cards_in_reading_order() {
        if let Some(path) = std::env::var_os("FUU_PREVIEW_DUMP") {
            std::fs::write(path, sample_pptx()).unwrap();
        }
        let out = preview(&sample_pptx()).unwrap();
        let h = &out.html;
        assert!(h.contains("2 枚のスライド"), "{h}");
        let title = h.find("<h2>新メール基盤の提案</h2>").expect(h);
        let body = h.find("背景").expect(h);
        assert!(title < body, "title first: {h}");
        assert!(h.contains("<b><span style=\"color: #1f4e79; font-size: 18pt;\">背景</span></b>"), "{h}");
        assert!(h.contains("margin-left: 26px;\">•\u{2002}現行システムの課題 &lt;x&gt;"), "{h}");
        assert_eq!(out.images.len(), 1);
        assert!(h.contains("width=\"300\""), "{h}");
        assert!(h.contains("外部から読み込む画像 1 件"), "{h}");
        assert!(h.contains("<td colspan=\"2\"><p>費用</p></td>") && !h.contains(">x<"), "{h}");
        assert!(h.contains("¥1,200,000"));
    }

    #[test]
    fn not_a_presentation() {
        assert!(preview(b"nope").is_err());
        assert!(preview(&package(&[("ppt/other.xml", b"<x/>")])).is_err());
    }
}
