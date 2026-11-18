//! Layout, painting and geometry for the rich editor (parley + vello_cpu).
//!
//! The document is laid out with parley at the editor width; glyphs and text
//! decorations are painted into a premultiplied RGBA buffer the host shows as an
//! image. The caret and selection are geometry for the host to draw on top, so
//! moving the caret never repaints the text. While an IME composition is active
//! its text is inserted (underlined) at the caret for display only.

use std::ops::Range;

use anyrender::{render_to_buffer, Glyph, PaintScene};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use kurbo::{Affine, Rect};
use parley::{
    Affinity, Alignment, AlignmentOptions, Cursor, FontContext, FontWeight, GenericFamily, IndentOptions,
    Layout, LayoutContext, LineHeight, PositionedLayoutItem, Selection, StyleProperty,
};
use peniko::{Color, Fill};

use crate::model::{Block, RichDoc};

/// Text colour carried through the layout (parley "brush"), plus whether the run
/// is drawn slanted. Italic is painted as an oblique of the upright face instead of
/// asking font matching for an italic face: CJK fonts have none, and matching then
/// falls back to a lighter face.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Ink(pub [u8; 4], pub bool);

impl Ink {
    fn colour(self) -> Color {
        Color::from_rgba8(self.0[0], self.0[1], self.0[2], self.0[3])
    }

    pub fn from_rgb(rgb: u32) -> Self {
        Ink([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255], false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub text: Ink,
    pub muted: Ink,
    pub link: Ink,
    pub quote_bar: Ink,
    /// Base font size in logical pixels.
    pub font_size: f32,
}

impl Palette {
    pub fn light() -> Self {
        Palette {
            text: Ink::from_rgb(0x1c1c1a),
            muted: Ink::from_rgb(0x8a877f),
            link: Ink::from_rgb(0x3b4fd1),
            quote_bar: Ink::from_rgb(0xd9d6cf),
            font_size: 14.0,
        }
    }

    pub fn dark() -> Self {
        Palette {
            text: Ink::from_rgb(0xececef),
            muted: Ink::from_rgb(0x8e8e95),
            link: Ink::from_rgb(0x9aa6ff),
            quote_bar: Ink::from_rgb(0x3a3a40),
            font_size: 14.0,
        }
    }
}

/// A rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rectf {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct Painted {
    pub width: u32,
    pub height: u32,
    pub rgba_premultiplied: Vec<u8>,
}

/// One hard line of the document, laid out on its own so lists and quotes can
/// have a hanging indent.
struct Para {
    /// Document range of the line (without its newline).
    start: usize,
    end: usize,
    /// Text laid out: the line plus any IME composition, or a zero-width space
    /// for an empty line (parley needs a character to give the line a height).
    text: String,
    placeholder: bool,
    /// IME composition inside this line: (local offset, length).
    preedit: Option<(usize, usize)>,
    kind: Block,
    layout: Layout<Ink>,
    /// Top edge in layout units (physical pixels).
    y: f32,
}

impl Para {
    fn height(&self) -> f32 {
        self.layout.height()
    }

    /// Document offset → local display offset.
    fn local(&self, offset: usize) -> usize {
        if self.placeholder {
            return 0;
        }
        let o = offset.saturating_sub(self.start).min(self.end - self.start);
        match self.preedit {
            Some((at, len)) if o >= at => o + len,
            _ => o,
        }
    }

    /// Local display offset → document offset (inside a composition maps to its start).
    fn doc(&self, local: usize) -> usize {
        if self.placeholder {
            return self.start;
        }
        let o = match self.preedit {
            Some((at, len)) if local >= at + len => local - len,
            Some((at, _)) if local > at => at,
            _ => local,
        };
        (self.start + o).min(self.end)
    }

    fn cursor(&self, offset: usize) -> Cursor {
        Cursor::from_byte_index(&self.layout, self.local(offset).min(self.text.len()), Affinity::Downstream)
    }
}

pub struct EditorView {
    font_cx: FontContext,
    layout_cx: LayoutContext<Ink>,
    paras: Vec<Para>,
    scale: f32,
    width: f32,
    palette: Palette,
}

impl Default for EditorView {
    fn default() -> Self {
        Self::new()
    }
}

const TRANSPARENT: Ink = Ink([0, 0, 0, 0], false);
/// Slant of the painted oblique (degrees).
const OBLIQUE_DEGREES: f64 = 12.0;

#[allow(clippy::too_many_arguments)]
fn build_para(
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<Ink>,
    doc: &RichDoc,
    range: Range<usize>,
    text: &str,
    preedit: Option<(usize, usize)>,
    placeholder: bool,
    kind: Block,
    prefix: usize,
    width: f32,
    scale: f32,
    palette: Palette,
) -> Layout<Ink> {
    // Document range → local display range (empty when outside this line).
    let local = |r: &Range<usize>| -> Option<Range<usize>> {
        if placeholder {
            return None;
        }
        let s = r.start.max(range.start);
        let e = r.end.min(range.end);
        if s >= e {
            return None;
        }
        let map = |o: usize| match preedit {
            Some((at, len)) if o - range.start >= at => o - range.start + len,
            _ => o - range.start,
        };
        Some(map(s)..map(e))
    };
    let base = palette.font_size;
    let mut builder = layout_cx.ranged_builder(font_cx, text, scale, true);
    builder.push_default(StyleProperty::from(GenericFamily::SansSerif));
    builder.push_default(StyleProperty::FontSize(base));
    builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(1.6)));
    builder.push_default(StyleProperty::Brush(if placeholder { TRANSPARENT } else { palette.text }));
    let prefix_range = range.start..range.start + prefix;
    match kind {
        Block::Quote => {
            if let Some(r) = local(&range) {
                builder.push(StyleProperty::Brush(palette.muted), r);
            }
        }
        Block::Bullet | Block::Number => {
            if let Some(r) = local(&prefix_range) {
                builder.push(StyleProperty::Brush(palette.muted), r);
            }
        }
        Block::Rule | Block::Paragraph => {}
    }
    for run in doc.runs() {
        let Some(r) = local(&run.range) else { continue };
        let s = &run.style;
        if s.bold {
            builder.push(StyleProperty::FontWeight(FontWeight::BOLD), r.clone());
        }
        if s.underline || s.link.is_some() {
            builder.push(StyleProperty::Underline(true), r.clone());
        }
        if s.strike {
            builder.push(StyleProperty::Strikethrough(true), r.clone());
        }
        if s.size != crate::model::Size::Normal {
            builder.push(StyleProperty::FontSize(base * s.size.scale()), r.clone());
        }
        let colour = if s.link.is_some() {
            Some(palette.link)
        } else {
            s.colour.map(Ink::from_rgb)
        };
        if colour.is_some() || s.italic {
            // Italic needs the run's resolved colour, since the brush carries both.
            let base = if kind == Block::Quote { palette.muted } else { palette.text };
            let Ink(rgba, _) = colour.unwrap_or(base);
            builder.push(StyleProperty::Brush(Ink(rgba, s.italic)), r.clone());
        }
    }
    // The quote marker and a rule's character are drawn as shapes instead.
    if matches!(kind, Block::Quote | Block::Rule) {
        let hidden = if kind == Block::Rule { range.clone() } else { prefix_range.clone() };
        if let Some(r) = local(&hidden) {
            builder.push(StyleProperty::Brush(TRANSPARENT), r);
        }
    }
    if let Some((at, len)) = preedit {
        builder.push(StyleProperty::Underline(true), at..at + len);
    }
    let mut layout = Layout::new();
    builder.build_into(&mut layout, text);
    let max = Some(width * scale);
    layout.break_all_lines(max);
    if prefix > 0 && matches!(kind, Block::Bullet | Block::Number | Block::Quote) && !placeholder {
        // Hanging indent: wrapped lines start where the text after the marker does.
        let at = preedit.map_or(prefix, |(p, len)| if prefix >= p { prefix + len } else { prefix }).min(text.len());
        let indent = Cursor::from_byte_index(&layout, at, Affinity::Downstream).geometry(&layout, 0.0).x0 as f32;
        if indent > 0.0 {
            layout.set_text_indent(indent, IndentOptions { each_line: false, hanging: true });
            layout.break_all_lines(max);
        }
    }
    layout.align(Alignment::Start, AlignmentOptions::default());
    layout
}

impl EditorView {
    pub fn new() -> Self {
        EditorView {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
            paras: Vec::new(),
            scale: 1.0,
            width: 600.0,
            palette: Palette::light(),
        }
    }

    /// Lay the document out at `width` logical pixels.
    pub fn relayout(&mut self, doc: &RichDoc, preedit: Option<(usize, &str)>, width: f32, scale: f32, palette: Palette) {
        self.scale = scale.max(0.5);
        self.width = width.max(40.0);
        self.palette = palette;
        let mut paras = Vec::new();
        let mut y = 0.0;
        let mut start = 0;
        let mut preedit = preedit.filter(|(_, t)| !t.is_empty());
        for line in doc.text().split('\n') {
            let range = start..start + line.len();
            start = range.end + 1;
            let (kind, prefix) = RichDoc::block_of(line);
            let mut text = line.to_owned();
            let mut local_preedit = None;
            if let Some((at, composing)) = preedit {
                if at >= range.start && at <= range.end && text.is_char_boundary(at - range.start) {
                    text.insert_str(at - range.start, composing);
                    local_preedit = Some((at - range.start, composing.len()));
                    preedit = None;
                }
            }
            let placeholder = text.is_empty();
            if placeholder {
                text.push('\u{200B}');
            }
            let layout = build_para(
                &mut self.font_cx,
                &mut self.layout_cx,
                doc,
                range.clone(),
                &text,
                local_preedit,
                placeholder,
                kind,
                prefix,
                self.width,
                self.scale,
                palette,
            );
            let para = Para { start: range.start, end: range.end, text, placeholder, preedit: local_preedit, kind, layout, y };
            y += para.height();
            paras.push(para);
        }
        self.paras = paras;
    }

    /// Content height in logical pixels (at least one line).
    pub fn height(&self) -> f32 {
        let total = self.paras.last().map_or(0.0, |p| p.y + p.height());
        (total / self.scale).max(self.palette.font_size * 1.6)
    }

    fn doc_len(&self) -> usize {
        self.paras.last().map_or(0, |p| p.end)
    }

    /// Paint glyphs, decorations, quote bars and rules (transparent background).
    pub fn paint(&self) -> Painted {
        let width = (self.width * self.scale).ceil().max(1.0) as u32;
        let height = (self.height() * self.scale).ceil().max(1.0) as u32;
        let s = f64::from(self.scale);
        let bar = self.palette.quote_bar.colour();
        let rgba = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| {
                // One continuous bar per run of consecutive quote lines.
                let mut i = 0;
                while i < self.paras.len() {
                    if self.paras[i].kind != Block::Quote {
                        i += 1;
                        continue;
                    }
                    let top = self.paras[i].y;
                    let mut j = i;
                    while j + 1 < self.paras.len() && self.paras[j + 1].kind == Block::Quote {
                        j += 1;
                    }
                    let bottom = self.paras[j].y + self.paras[j].height();
                    let inset = 3.0 * s;
                    let rect = Rect::new(1.0 * s, f64::from(top) + inset, 4.0 * s, f64::from(bottom) - inset);
                    scene.fill(Fill::NonZero, Affine::IDENTITY, bar, None, &rect);
                    i = j + 1;
                }
                for p in &self.paras {
                    let transform = Affine::translate((0.0, f64::from(p.y)));
                    if p.kind == Block::Rule {
                        let mid = f64::from(p.y + p.height() / 2.0);
                        let rect = Rect::new(0.0, mid, f64::from(width), mid + s.max(1.0));
                        scene.fill(Fill::NonZero, Affine::IDENTITY, bar, None, &rect);
                    }
                    for line in p.layout.lines() {
                        for item in line.items() {
                            let PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };
                            let style = glyph_run.style();
                            if style.brush.0[3] == 0 {
                                continue;
                            }
                            let run = glyph_run.run();
                            let colour = style.brush.colour();
                            let coords: Vec<i16> = run.normalized_coords().iter().map(|c| c.to_bits()).collect();
                            let glyph_transform = style.brush.1.then(|| Affine::skew(OBLIQUE_DEGREES.to_radians().tan(), 0.0));
                            scene.draw_glyphs(
                                run.font(),
                                run.font_size(),
                                true,
                                &coords,
                                kurbo::Vec2::ZERO,
                                Fill::NonZero,
                                colour,
                                1.0,
                                transform,
                                glyph_transform,
                                glyph_run.positioned_glyphs().map(|g| Glyph { id: g.id as _, x: g.x, y: g.y }),
                            );
                            let metrics = run.font_metrics();
                            let x0 = f64::from(glyph_run.offset());
                            let x1 = x0 + f64::from(glyph_run.advance());
                            let baseline = f64::from(glyph_run.baseline());
                            let mut decorate = |offset: f32, size: f32| {
                                let y = baseline - f64::from(offset);
                                let h = f64::from(size.max(1.0));
                                scene.fill(Fill::NonZero, transform, colour, None, &Rect::new(x0, y, x1, y + h));
                            };
                            if style.underline.is_some() {
                                decorate(metrics.underline_offset, metrics.underline_size);
                            }
                            if style.strikethrough.is_some() {
                                decorate(metrics.strikethrough_offset, metrics.strikethrough_size);
                            }
                        }
                    }
                }
            },
            width,
            height,
        );
        Painted { width, height, rgba_premultiplied: rgba }
    }

    fn para_of(&self, offset: usize) -> Option<&Para> {
        self.paras.iter().rev().find(|p| p.start <= offset).or(self.paras.first())
    }

    /// Paragraph under a vertical position in layout units.
    fn para_at_y(&self, y: f32) -> Option<&Para> {
        self.paras.iter().rev().find(|p| p.y <= y).or(self.paras.first())
    }

    fn rect(&self, b: parley::BoundingBox, y: f32) -> Rectf {
        let s = f64::from(self.scale);
        Rectf {
            x: (b.x0 / s) as f32,
            y: ((b.y0 + f64::from(y)) / s) as f32,
            w: ((b.x1 - b.x0) / s) as f32,
            h: ((b.y1 - b.y0) / s) as f32,
        }
    }

    /// Caret rectangle for a document offset (at the end of an active composition).
    pub fn caret(&self, offset: usize) -> Rectf {
        let line = self.palette.font_size * 1.6;
        let Some(p) = self.para_of(offset) else { return Rectf { x: 0.0, y: 0.0, w: 1.5, h: line } };
        let o = offset.saturating_sub(p.start).min(p.end - p.start);
        let index = if p.placeholder {
            0
        } else {
            match p.preedit {
                Some((at, len)) if o >= at => o + len,
                _ => o,
            }
        };
        let cursor = Cursor::from_byte_index(&p.layout, index.min(p.text.len()), Affinity::Downstream);
        let mut r = self.rect(cursor.geometry(&p.layout, 1.5 * self.scale), p.y);
        if r.h <= 0.0 {
            r.h = line;
        }
        r.w = 1.5;
        r
    }

    /// Selection rectangles between two document offsets.
    pub fn selection_rects(&self, anchor: usize, focus: usize) -> Vec<Rectf> {
        let (lo, hi) = (anchor.min(focus), anchor.max(focus));
        if lo == hi {
            return Vec::new();
        }
        let mut out = Vec::new();
        for p in self.paras.iter().filter(|p| p.end >= lo && p.start <= hi) {
            let (a, b) = (lo.max(p.start), hi.min(p.end));
            if a < b {
                let sel = Selection::new(p.cursor(a), p.cursor(b));
                out.extend(sel.geometry(&p.layout).into_iter().map(|(bb, _)| self.rect(bb, p.y)));
            }
            if hi > p.end {
                // The line break itself is selected.
                let c = self.caret(p.end);
                out.push(Rectf { x: c.x, y: c.y, w: self.palette.font_size * 0.4, h: c.h });
            }
        }
        out
    }

    /// Document offset under a point (logical pixels).
    pub fn hit(&self, x: f32, y: f32) -> usize {
        let (x, y) = (x * self.scale, y * self.scale);
        let Some(p) = self.para_at_y(y) else { return 0 };
        p.doc(Cursor::from_point(&p.layout, x, y - p.y).index())
    }

    /// Word under a point, as a document range.
    pub fn word_at(&self, x: f32, y: f32) -> Range<usize> {
        let (x, y) = (x * self.scale, y * self.scale);
        let Some(p) = self.para_at_y(y) else { return 0..0 };
        let r = Selection::word_from_point(&p.layout, x, y - p.y).text_range();
        p.doc(r.start)..p.doc(r.end)
    }

    /// Whole line (paragraph) under a point.
    pub fn line_at(&self, _x: f32, y: f32) -> Range<usize> {
        self.para_at_y(y * self.scale).map_or(0..0, |p| p.start..p.end)
    }

    /// Move by visual lines (Up/Down), optionally extending the selection.
    pub fn move_lines(&self, anchor: usize, focus: usize, delta: isize, extend: bool) -> (usize, usize) {
        let mut f = focus;
        for _ in 0..delta.unsigned_abs() {
            let c = self.caret(f);
            let target = if delta > 0 { c.y + c.h + 1.0 } else { c.y - 1.0 };
            if target < 0.0 {
                f = 0;
                break;
            }
            if target >= self.height() {
                f = self.doc_len();
                break;
            }
            f = self.hit(c.x, target);
        }
        (if extend { anchor } else { f }, f)
    }

    fn line_edge(&self, anchor: usize, focus: usize, extend: bool, end: bool) -> (usize, usize) {
        let Some(p) = self.para_of(focus) else { return (anchor, focus) };
        let c = p.cursor(focus);
        let sel = Selection::new(c, c);
        let moved = if end { sel.line_end(&p.layout, false) } else { sel.line_start(&p.layout, false) };
        let f = p.doc(moved.focus().index());
        (if extend { anchor } else { f }, f)
    }

    pub fn line_start(&self, anchor: usize, focus: usize, extend: bool) -> (usize, usize) {
        self.line_edge(anchor, focus, extend, false)
    }

    pub fn line_end(&self, anchor: usize, focus: usize, extend: bool) -> (usize, usize) {
        self.line_edge(anchor, focus, extend, true)
    }

    /// Number of visual lines on one page of `height` logical pixels.
    pub fn lines_per_page(&self, height: f32) -> isize {
        ((height / (self.palette.font_size * 1.6)).floor() as isize - 1).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_of(text: &str, width: f32) -> (RichDoc, EditorView) {
        let doc = RichDoc::from_plain(text);
        let mut v = EditorView::new();
        v.relayout(&doc, None, width, 1.0, Palette::light());
        (doc, v)
    }

    #[test]
    fn lays_out_and_paints() {
        let (_, v) = view_of("こんにちは\n2 行目 with English", 300.0);
        assert!(v.height() > 30.0, "two lines: {}", v.height());
        let p = v.paint();
        assert_eq!(p.rgba_premultiplied.len(), (p.width * p.height * 4) as usize);
        assert!(p.rgba_premultiplied.chunks(4).any(|px| px[3] > 0), "glyphs were painted");
    }

    #[test]
    fn hit_and_caret_round_trip() {
        let (doc, v) = view_of("abc\ndef", 300.0);
        let caret = v.caret(5); // between d and e
        let back = v.hit(caret.x + 0.5, caret.y + caret.h / 2.0);
        assert_eq!(back, 5);
        assert!(v.caret(4).y > v.caret(0).y, "second line is lower");
        assert_eq!(v.move_lines(1, 1, 1, false), (5, 5));
        assert_eq!(v.line_end(0, 0, false).1, 3);
        assert_eq!(v.line_start(6, 6, true), (6, 4));
        assert!(v.selection_rects(0, doc.len()).len() >= 2);
    }

    #[test]
    fn preedit_shifts_display_but_not_document() {
        let doc = RichDoc::from_plain("ab");
        let mut v = EditorView::new();
        v.relayout(&doc, Some((1, "にほん")), 300.0, 1.0, Palette::light());
        assert!(v.caret(1).x > v.caret(0).x + 10.0, "caret after the composition");
        let end = v.caret(2);
        assert_eq!(v.hit(end.x + 1.0, end.y + end.h / 2.0), 2);
    }

    #[test]
    fn quote_and_list_lines_get_a_hanging_indent() {
        let text = format!("{}{}", crate::model::QUOTE, "word ".repeat(40));
        let (_, v) = view_of(&text, 200.0);
        let first = v.caret(crate::model::QUOTE.len());
        let wrapped = v.caret(text.len());
        assert!(wrapped.y > first.y, "wraps");
        let continuation = v.caret(v.line_start(text.len(), text.len(), false).1);
        assert!(continuation.y > first.y && (continuation.x - first.x).abs() < 1.0, "continuation indented: {continuation:?} vs {first:?}");
        let (_, empty) = view_of("a\n\nb", 300.0);
        assert!(empty.caret(2).y > empty.caret(0).y && empty.caret(3).y > empty.caret(2).y, "empty lines have height");
        assert_eq!(empty.move_lines(0, 0, 2, false).1, 3);
    }

    #[test]
    fn wraps_long_lines() {
        let (_, narrow) = view_of(&"word ".repeat(60), 120.0);
        let (_, wide) = view_of(&"word ".repeat(60), 1000.0);
        assert!(narrow.height() > wide.height() * 3.0);
    }
}
